"""Build review sheets from verify-rvm-quality.mjs, using public footage only."""
from pathlib import Path
import json
import sys
import numpy as np
from PIL import Image, ImageDraw, ImageFont
root = Path('/tmp/matting-bakeoff')
out = Path('/tmp/engine-shots/v3') / (sys.argv[1] if len(sys.argv)>1 else 'quality')
crops = json.loads((root/'crops.json').read_text())
font = ImageFont.truetype('/System/Library/Fonts/Helvetica.ttc', 17)
for clip in (sys.argv[2:] or ['hair','curly','wave','glasses','chair']):
    indices = [1,31,61,90] if clip == 'wave' else [1,26,51,75]
    sheet = Image.new('RGB',(4*384,4*242),(24,24,26))
    edges = Image.new('RGB',(8*224,3*250),(24,24,26))
    for col,index in enumerate(indices):
        stem=f'{index:05d}.png'
        rgb=Image.open(root/'frames'/clip/stem).convert('RGB')
        old=Image.open(out/clip/('old-'+stem)).convert('L')
        alpha=Image.open(out/clip/('rvm-'+stem)).convert('L')
        low=np.array(alpha);low[low<8]=0
        threshold=Image.fromarray(low)
        bg=Image.new('RGB',rgb.size,(80,184,120))
        images=[rgb,Image.composite(rgb,bg,old),Image.composite(rgb,bg,alpha),Image.composite(rgb,bg,threshold)]
        for row,(im,label) in enumerate(zip(images,['Original','Old MediaPipe','RVM direct','RVM alpha < 3% removed'])):
            sheet.paste(im.resize((384,216)),(col*384,row*242+26))
            ImageDraw.Draw(sheet).text((col*384+6,row*242+4),f'{label} · frame {index}',fill='white',font=font)
        for ci,(x,y) in enumerate(crops[clip][str(index)]):
            box=(x,y,x+112,y+112)
            for row,(im,label) in enumerate(zip(images[1:],['Old MediaPipe','RVM direct','RVM < 3% removed'])):
                xx=(col*2+ci)*224
                edges.paste(im.crop(box).resize((224,224)),(xx,row*250+26))
                ImageDraw.Draw(edges).text((xx+4,row*250+4),f'{label} · {index}',fill='white',font=font)
    sheet.save(out/(clip+'-contact.png'));edges.save(out/(clip+'-edges.png'))
    print(clip)
