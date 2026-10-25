"""Download public stock sources and prepare explicitly labeled derived scenes."""
from pathlib import Path
import hashlib,json,subprocess,urllib.request
import imageio_ffmpeg
import numpy as np
from PIL import Image
r=Path('/tmp/engine-shots/v4');r.mkdir(parents=True,exist_ok=True)
sources={
 'office':{'id':8865594,'creator':'Yan Krukau','url':'https://www.pexels.com/video/man-sitting-on-a-chair-while-working-8865594/','download':'https://videos.pexels.com/video-files/8865594/8865594-hd_1080_1920_25fps.mp4','start':2,'filter':'fps=25,scale=1280:-2,crop=1280:720:0:600','note':'Bright wall, dark office chair, hand at headset. Portrait source reframed to show face/hand; not a high headrest scene.'},
 'spin':{'id':9065728,'creator':'cottonbro studio','url':'https://www.pexels.com/video/a-man-sitting-on-a-spinning-office-chair-9065728/','download':'https://videos.pexels.com/video-files/9065728/9065728-hd_1366_720_25fps.mp4','start':7,'filter':'fps=25,scale=1280:720:force_original_aspect_ratio=increase,crop=1280:720','note':'Lower-body chair rotation on pale studio wall; tests a partial person. Does not show a head.'}}
for name,s in sources.items():
 f=r/(name+'-original.mp4')
 if not f.exists():urllib.request.urlretrieve(s['download'],f)
 s['sha256']=hashlib.sha256(f.read_bytes()).hexdigest();p=r/'frames'/name;p.mkdir(parents=True,exist_ok=True)
 subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(),'-y','-ss',str(s['start']),'-i',str(f),'-t','3','-vf',s['filter'],'-loglevel','error',str(p/'%05d.png')],check=True)
for name in ['chair-close','bright-chair']:
 p=r/'frames'/name;p.mkdir(parents=True,exist_ok=True)
 for f in sorted(Path('/tmp/matting-bakeoff/frames/chair').glob('*.png')):
  im=Image.open(f).convert('RGB').crop((190,30,830,390)).resize((1280,720),Image.Resampling.LANCZOS)
  if name=='bright-chair':
   a=np.array(im).astype(float);b=a[:,:,2];g=a[:,:,1];red=a[:,:,0]
   wall=(b>red+5)&(g>red+2)&(a.min(axis=2)>85);mix=np.clip((a.min(axis=2)-85)/40,0,1)*wall
   im=Image.fromarray((a*(1-mix[:,:,None])+245*mix[:,:,None]).astype('uint8'))
  im.save(p/f.name)
(r/'sources.json').write_text(json.dumps(sources,indent=2))
(r/'SOURCES.md').write_text('''# Public footage used in v4

Existing chair, curly, wave, glasses, hair and exit sources/licenses:
/tmp/matting-bakeoff/SOURCES.md. All use the Pexels License, not CC0.
All visible people are foreground. No maintainer footage is used.

New sources (downloaded 2026-10-23), also Pexels License:
https://www.pexels.com/license/

'''+''.join(f"## {n}\n\nCreator: {s['creator']}\n\n{s['url']}\n\nDirect download: {s['download']}\n\nSHA-256: `{s['sha256']}`\n\nExtract {s['start']}–{s['start']+3}s, 25fps. {s['note']}\n\n"for n,s in sources.items())+'''## Derived stress scenes

chair-close: crop original public chair frames (190,30)–(830,390), resize to
1280x720. No segmentation or generated person is used.
bright-chair: same crop, then raise blue/green pale wall pixels toward RGB 245
using the explicit color rule in prepare-hybrid-fixtures.py. This is an altered
stress scene, not unmodified public footage or an exact reproduction of the
reported MacBook scene.
wave-fast: process every second source frame at a 30fps timeline, giving 2x
motion. Source frames remain public, unmodified RGB; no frame interpolation.
exit shows a person partly turned away. spin shows only lower-body rotation.
''')
