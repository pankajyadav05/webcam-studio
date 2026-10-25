// Preview media is session-only. No storage, file writes, audio tracks or webview networking.
export const RECORD_MS = 5000;
const MAX_BYTES = 25 * 1024 * 1024;
export function recordingMime(Recorder = globalThis.MediaRecorder) {
  if (!Recorder) throw new Error('Recording is unavailable in this web view. Try Sample instead.');
  const type = ['video/mp4;codecs=avc1.42E01E', 'video/mp4', 'video/webm;codecs=vp8', 'video/webm']
    .find(value => Recorder.isTypeSupported(value));
  if (!type) throw new Error('This web view has no supported recording format. Try Sample instead.');
  return type;
}
export function createPreviewSources({ camera, invoke, onChange, onSource,
  makeVideo = () => document.createElement('video'), urls = URL,
  Recorder = globalThis.MediaRecorder, makeStream = tracks => new MediaStream(tracks),
  duration = RECORD_MS, now = () => performance.now() }) {
  let state = { selected:'camera', active:'camera', status:'idle', error:'', hasClip:false,
    recording:false, progress:0, manifest:null, clipId:'wave', activeSampleId:null,
    listing:'idle', listError:'' };
  let token = 0, disposed = false, myClip = null, sample = null, recording = null;
  const emit = patch => { state = { ...state, ...patch }; if (!disposed) onChange?.({ ...state }); };
  const release = item => { if (item) { item.video.pause(); item.video.removeAttribute('src'); item.video.load(); urls.revokeObjectURL(item.url); } };
  async function decode(blob) {
    const url = urls.createObjectURL(blob), video = makeVideo();
    video.muted = true; video.loop = true; video.playsInline = true; video.preload = 'auto';
    video.src = url;
    let timeout;
    try {
      await Promise.race([video.play(), new Promise((_, reject) => {
        timeout = setTimeout(() => reject(new Error('The clip could not start. Try again.')), 15000);
      })]);
      return { video, url };
    } catch (error) { release({ video, url }); throw error; }
    finally { clearTimeout(timeout); }
  }
  function activate(kind, item) {
    if (state.active === 'clip' && kind !== 'clip') myClip?.video.pause();
    if (state.active === 'sample' && kind !== 'sample') sample?.video.pause();
    onSource?.(item?.video || camera);
    emit({ active:kind, activeSampleId:kind === 'sample' ? item.id : null, status:'idle', error:'' });
  }
  function cancelRecording() {
    if (!recording) return;
    recording.cancelled = true;
    clearTimeout(recording.stopTimer); clearInterval(recording.progressTimer);
    if (recording.recorder.state !== 'inactive') recording.recorder.stop();
    recording = null; emit({ recording:false, progress:0 });
  }
  async function select(kind) {
    if (!['camera','clip','sample'].includes(kind)) throw new RangeError('Unknown preview source');
    if (disposed) return;
    const request = ++token;
    if (kind !== 'clip') cancelRecording();
    emit({ selected:kind, error:'', status:'idle' });
    if (kind === 'camera') { activate('camera'); return; }
    if (kind === 'clip') {
      if (!myClip) { activate('camera'); return; }
      try { await myClip.video.play(); if (request === token) activate('clip',myClip); }
      catch { if (request === token) emit({error:'Your clip could not play. Record again.'}); }
      return;
    }
    emit({ status:'loading' });
    try {
      let manifest = state.manifest;
      if (!manifest) manifest = await invoke('sample_manifest');
      if (request !== token) return;
      if (!manifest?.clips?.length) throw new Error('No samples are available yet. Try again later.');
      const clip = manifest.clips.find(c => c.id === state.clipId) || manifest.clips[0];
      emit({ manifest, clipId:clip.id });
      if (sample?.id === clip.id) {
        await sample.video.play(); if (request === token) activate('sample',sample); return;
      }
      const bytes = await invoke('sample_clip', { id:clip.id });
      if (request !== token) return;
      const item = await decode(new Blob([bytes], {type:'video/mp4'}));
      if (request !== token) { release(item); return; }
      const old = sample;
      sample = { ...item, id:clip.id };
      activate('sample',sample); release(old);
      emit({ manifest:{...manifest, clips:manifest.clips.map(c => c.id === clip.id ? {...c,cached:true} : c)} });
    } catch (error) {
      if (request === token) emit({ status:'error', error:`Sample unavailable. ${error.message || error} You can use Camera or My clip, or try again.` });
    }
  }
  return {
    get: () => ({...state}),
    get video() { return state.active === 'clip' ? myClip.video : state.active === 'sample' ? sample.video : camera; },
    select,
    chooseClip(id) {
      if (!state.manifest?.clips.some(c => c.id === id)) return Promise.reject(new RangeError('Unknown sample'));
      emit({clipId:id}); return select('sample');
    },
    // Lists the clips (a small manifest) without downloading any of them.
    async listSamples() {
      if (state.manifest || disposed) return state.manifest;
      emit({ listing:'loading', listError:'' });
      try {
        const manifest = await invoke('sample_manifest');
        if (disposed) return null;
        if (!manifest?.clips?.length) throw new Error('No samples are available yet. Try again later.');
        // A sample download may have filled this in meanwhile, with fresher cache flags.
        if (!state.manifest) emit({ manifest });
        emit({ listing:'ready' });
        return state.manifest;
      } catch (error) {
        if (!disposed) emit({ listing:'error', listError:`Samples could not be listed. ${error.message || error}` });
        return null;
      }
    },
    retry() { return select(state.selected); },
    record() {
      if (disposed || recording || state.selected !== 'clip') return;
      try {
        const tracks = camera.srcObject?.getVideoTracks().filter(track => track.readyState === 'live');
        if (!tracks?.length) throw new Error('Open your camera before recording.');
        const mimeType = recordingMime(Recorder);
        // A stream made from the camera video tracks cannot capture microphone audio.
        const recorder = new Recorder(makeStream(tracks), {mimeType,videoBitsPerSecond:6000000});
        const job = { recorder, cancelled:false, chunks:[], size:0, stopTimer:null, progressTimer:null };
        recording = job;
        ++token; activate('camera'); emit({recording:true,progress:0});
        recorder.ondataavailable = event => {
          if (job.cancelled || !event.data?.size) return;
          job.size += event.data.size;
          if (job.size > MAX_BYTES) {
            job.cancelled = true; if (recorder.state !== 'inactive') recorder.stop();
            emit({error:'The recording was too large. Try recording again.'});
          } else job.chunks.push(event.data);
        };
        recorder.onerror = () => { job.cancelled = true; emit({error:'Recording failed. Try recording again.'}); if (recorder.state !== 'inactive') recorder.stop(); };
        recorder.onstop = async () => {
          clearTimeout(job.stopTimer); clearInterval(job.progressTimer);
          if (recording === job) { recording = null; emit({recording:false,progress:0}); }
          if (job.cancelled || disposed) return;
          const request = token;
          try {
            if (!job.size) throw new Error('The camera returned an empty recording. Try again.');
            const item = await decode(new Blob(job.chunks,{type:recorder.mimeType || mimeType}));
            if (disposed || request !== token) { release(item); return; }
            const old = myClip; myClip = item; emit({hasClip:true});
            activate('clip',myClip); release(old);
          } catch (error) { if (request === token) emit({error:error.message}); }
        };
        recorder.start(250);
        const start = now();
        job.stopTimer = setTimeout(() => { if (recorder.state !== 'inactive') recorder.stop(); },duration);
        job.progressTimer = setInterval(() => emit({progress:Math.min(5,(now()-start)/duration*5)}),100);
      } catch (error) { recording = null; emit({recording:false,error:error.message}); }
    },
    dispose() { disposed = true; token++; cancelRecording(); release(myClip); release(sample); myClip = sample = null; },
  };
}
