// Deliberately controlled confidence maps test the compositor independently of
// whether the real model happens to misclassify this particular chair frame.
import { PERSON_OUTLINE } from './matte-fixture.js';
export function makeSyntheticSegmenter() {
  const canvas=new OffscreenCanvas(256,256), ctx=canvas.getContext('2d');
  ctx.scale(256/1280,256/720);ctx.fillStyle='white';ctx.fill(new Path2D(PERSON_OUTLINE));
  const rgba=ctx.getImageData(0,0,256,256).data, values=new Float32Array(256*256);
  for(let i=0;i<values.length;i++) values[i]=rgba[i*4+3]/255;
  for(let y=0;y<256;y++)for(let x=0;x<256;x++){
    const sx=x*5,sy=y*720/256;
    if(sx>=495&&sx<530&&sy>=85&&sy<125)values[y*256+x]=.60;
    if(sx>=737&&sx<767&&sy>=100&&sy<130)values[y*256+x]=.35;
  }
  return {
    getLabels:()=>['person'],close(){},
    segmentForVideo(_bitmap,_timestamp,callback){callback({confidenceMasks:[{width:256,height:256,getAsFloat32Array:()=>values}]});},
  };
}
