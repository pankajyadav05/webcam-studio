"""Public-footage contact sheets; the leakage reference is a proxy, not truth."""
from pathlib import Path
import sys,json
import numpy as np
from PIL import Image,ImageDraw,ImageFont,ImageFilter
root=Path('/tmp/engine-shots/v4');folder=root/(sys.argv[1] if len(sys.argv)>1 else 'chrome-quality')
font=ImageFont.truetype('/System/Library/Fonts/Helvetica.ttc',16)
known=json.loads(Path('/tmp/matting-bakeoff/crops.json').read_text());metrics=[]
for clip in [x.name for x in folder.iterdir() if x.is_dir()]:
 ids=sorted(int(x.stem.split('-')[-1])for x in (folder/clip).glob('source-*.png'))
 if len(ids)<4:continue
 for view in ['replace','blur']:
  sheet=Image.new('RGB',(384*len(ids),3*242),'#202126');d=ImageDraw.Draw(sheet)
  for col,i in enumerate(ids):
   for row,name in enumerate(['rvm','mediapipe','hybrid']):
    im=Image.open(folder/clip/f'{name}-{view}-{i}.png').convert('RGB')
    sheet.paste(im.resize((384,216)),(384*col,242*row+26));d.text((384*col+6,242*row+4),f'{name} · {clip} · {i}',fill='white',font=font)
  sheet.save(folder/f'{clip}-{view}-contact.png')
 # Exact 4x nearest-neighbour crops make translucent pixels visible.
 edges=Image.new('RGB',(len(ids)*2*256,3*282),'#202126');d=ImageDraw.Draw(edges)
 for col,i in enumerate(ids):
  keys=known.get('wave' if clip=='wave-fast' else clip,{});k=min(keys,key=lambda k:abs(int(k)-(i*2+1 if clip=='wave-fast' else i+1))) if keys else None
  coords=keys[k] if k else ([(700,140),(450,490)]if clip=='office'else[(700,200),(900,350)])
  for ci,(x,y)in enumerate(coords):
   for row,name in enumerate(['rvm','mediapipe','hybrid']):
    im=Image.open(folder/clip/f'{name}-replace-{i}.png').convert('RGB')
    edges.paste(im.crop((x,y,x+64,y+64)).resize((256,256),Image.Resampling.NEAREST),((col*2+ci)*256,row*282+26));d.text(((col*2+ci)*256+5,row*282+4),f'{name} · {i} · {x},{y}',fill='white',font=font)
 edges.save(folder/f'{clip}-edges-4x.png')
 for i in ids:
  ref=np.asarray(Image.open(folder/clip/f'reference-{i}.png').resize((1280,720),Image.Resampling.BILINEAR))[:,:,2]/255
  outside=np.asarray(Image.fromarray(np.uint8(ref>=.65)*255).filter(ImageFilter.MaxFilter(49)))==0
  row={'clip':clip,'index':i,'outsidePixels':int(outside.sum())}
  rvm=np.asarray(Image.open(folder/clip/f'rvm-alpha-{i}.png').convert('L'))/255
  hybrid=np.asarray(Image.open(folder/clip/f'hybrid-alpha-{i}.png').convert('L'))/255
  for name in ['rvm','mediapipe','hybrid']:
   a=np.asarray(Image.open(folder/clip/f'{name}-alpha-{i}.png').convert('L'))/255
   row[name]=float(((a>.5)&outside).sum()/max(1,outside.sum())*100)
  # This measures gate attenuation relative to RVM in a reference-supported band.
  edge=(rvm>.02)&(rvm<.98)&(~outside)
  row['supportedEdgeMeanLoss']=float(np.maximum(0,rvm[edge]-hybrid[edge]).mean()) if edge.any()else 0
  row['supportedOpaqueLossPixels']=int(((rvm>.98)&(hybrid<.5)&(~outside)).sum())
  metrics.append(row)
 print(clip)
summary=[]
for clip in sorted(set(x['clip']for x in metrics)):
 rows=[x for x in metrics if x['clip']==clip and x['index']>0]
 summary.append({'clip':clip,'frames':len(rows),**{k:float(np.mean([r[k]for r in rows]))for k in ['rvm','mediapipe','hybrid','supportedEdgeMeanLoss','supportedOpaqueLossPixels']}})
(folder/'leakage.json').write_text(json.dumps({'reference':'MP confidence >= .65, bilinear upsampled, square dilation 24px. Fraction of outside pixels with alpha > .5, percent. Startup frame excluded from means.','frames':metrics,'summary':summary},indent=2));print(json.dumps(summary,indent=2))
