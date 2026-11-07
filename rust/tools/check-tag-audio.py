#!/usr/bin/env python3
"""Check copied synthetic metadata fixtures with an independent decoder."""
import hashlib,json,pathlib,subprocess,sys
root=pathlib.Path(sys.argv[1] if len(sys.argv)>1 else '/fixtures')
results=[]
for name,ext in [('aac','m4a'),('alac','m4a'),('mp3','mp3'),('flac','flac'),('ogg','ogg')]:
 before=root/f'{name}-original.{ext}';after=root/f'{name}-edited.{ext}'
 def audio(path):return subprocess.check_output(['ffmpeg','-v','error','-i',str(path),'-map','0:a:0','-f','hash','-hash','sha256','-'],text=True).strip()
 def tags(path):
  info=json.loads(subprocess.check_output(['ffprobe','-v','error','-show_entries','format_tags:stream_tags','-of','json',str(path)],text=True))
  out={}
  for source in [info.get('format',{})]+info.get('streams',[]):out.update({k.lower():v for k,v in source.get('tags',{}).items()})
  return out
 a,b=audio(before),audio(after)
 assert a==b,f'{name}: decoded audio changed'
 original,edited=tags(before),tags(after)
 for key in ['comment','description','composer','copyright']:
  if key in original:assert edited.get(key)==original[key],f'{name}: unrelated {key} changed: {original.get(key)!r}->{edited.get(key)!r}'
 def total(tags, key):
  value=tags.get(key,'')
  if '/' in value:return value.split('/')[-1]
  return next((tags[k] for k in [key+'total','total'+key+'s'] if k in tags),None)
 for key in ['track','disc']:
  if total(original,key) is not None:assert total(edited,key)==total(original,key),f'{name}: {key} total changed'
 results.append({'format':name,'decoded_hash':a,'unrelated_tags':'preserved','totals':'preserved'})
print(json.dumps(results,indent=2))
(root/'independent-audio-check.json').write_text(json.dumps(results,indent=2)+'\n')
