// Distances are measured in source pixels at 720p, independent of model size.
export const GATE = Object.freeze({ threshold: .65, core: .65, margin: 12, feather: 10,
  recoveryReach: 128, recoveryStep: 8, recoveryFeather: 16, highAlpha: .98, maxMotion: 96,
  staleMs: 250, rateHz: 15 });
const smooth = t => { t = Math.max(0, Math.min(1, t)); return t*t*(3-2*t); };
export function gateRamp(distance, margin = GATE.margin, feather = GATE.feather) {
  return 1 - smooth((distance-margin)/feather);
}
export function recoveryRamp(distance, alpha) {
  return gateRamp(distance, GATE.recoveryReach-GATE.recoveryFeather, GATE.recoveryFeather) * smooth((alpha-GATE.highAlpha)/.015);
}
// Chamfer distance has sub-texel diagonal support without a per-frame queue.
export function distanceToRegion(values, width, height, threshold, out = new Float32Array(values.length), aspect = 16/9) {
  const sx = 720*aspect/width, sy = 720/height, diagonal = Math.hypot(sx,sy), far = 4096;
  for (let i=0;i<out.length;i++) out[i] = values[i] >= threshold ? 0 : far;
  for (let y=0;y<height;y++) for (let x=0;x<width;x++) {
    const i=y*width+x; let d=out[i];
    if(x) d=Math.min(d,out[i-1]+sx);
    if(y) { d=Math.min(d,out[i-width]+sy); if(x)d=Math.min(d,out[i-width-1]+diagonal); if(x+1<width)d=Math.min(d,out[i-width+1]+diagonal); }
    out[i]=d;
  }
  for (let y=height-1;y>=0;y--) for (let x=width-1;x>=0;x--) {
    const i=y*width+x; let d=out[i];
    if(x+1<width) d=Math.min(d,out[i+1]+sx);
    if(y+1<height) { d=Math.min(d,out[i+width]+sy); if(x)d=Math.min(d,out[i+width-1]+diagonal); if(x+1<width)d=Math.min(d,out[i+width+1]+diagonal); }
    out[i]=d;
  }
  return out;
}
export function createGate(width,height,aspect = 16/9, reference = false) {
  let previous=null, previousTime=0, speed=0;
  const motion=new Float32Array(16*9), nextMotion=new Float32Array(16*9);
  const distance=new Float32Array(width*height), history=new Float32Array(width*height), histogram=new Uint32Array(97);
  return (values,timestamp,reset,pixels) => {
    distanceToRegion(values,width,height,GATE.threshold,distance,aspect);
    histogram.fill(0);nextMotion.fill(0); let changed=0, corePixels=0;
    for(let i=0;i<values.length;i++)if(values[i]>=GATE.core)corePixels++;
    if (!reset && previous && timestamp>previousTime) {
      for(let i=0;i<values.length;i++) if(values[i]>=GATE.core && previous[i]>0) { histogram[Math.min(96,Math.ceil(previous[i]))]++;changed++;
        const cell=Math.min(8,Math.floor(Math.floor(i/width)*9/height))*16+Math.min(15,Math.floor((i%width)*16/width));
        nextMotion[cell]=Math.max(nextMotion[cell],Math.min(1.2,previous[i]/Math.max(16,timestamp-previousTime))); }
      // A high percentile catches a moving hand without one noisy mask texel
      // widening the entire gate. Decay prevents old motion from lingering.
      let sum=0, displacement=0;
      for(let n=1;n<97;n++){sum+=histogram[n];if(sum>=changed*.98){displacement=n;break;}}
      speed=Math.max(speed*.65,changed>3?Math.min(1.2,displacement/Math.max(16,timestamp-previousTime)):0);
    } else {speed=0;motion.fill(0);}
    for(let i=0;i<motion.length;i++)motion[i]=Math.max(motion[i]*.65,nextMotion[i]);
    history.set(distance);previous=history;previousTime=timestamp;
    for(let i=0,j=0;i<values.length;i++,j+=4){
      pixels[j]=Math.round(255*gateRamp(distance[i]));
      // Preserve distance so the GPU can widen continuously rather than sample a sparse halo.
      pixels[j+1]=Math.round(255*Math.min(1,distance[i]/256));
      const cx=Math.min(15,Math.floor((i%width)*16/width)),cy=Math.min(8,Math.floor(Math.floor(i/width)*9/height));
      let localSpeed=0;
      for(let y=Math.max(0,cy-1);y<=Math.min(8,cy+1);y++)for(let x=Math.max(0,cx-1);x<=Math.min(15,cx+1);x++)localSpeed=Math.max(localSpeed,motion[y*16+x]);
      pixels[j+2]=Math.round(255*(reference?values[i]:localSpeed/1.2));pixels[j+3]=255;
    }
    return { speed, hasPerson: corePixels>0, ...GATE };
  };
}
