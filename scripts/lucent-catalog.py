#!/usr/bin/env python3
"""Wallhaven API / Alpha Coders public-page adapters. Output is typed JSON only."""
from html.parser import HTMLParser
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

CACHE = Path(os.environ.get('XDG_CACHE_HOME', str(Path.home()/'.cache')))/'lucent/wallpapers'
LIBRARY = Path.home()/'Pictures/Wallpapers/Lucent'
AGENT = 'LucentWallpaperBrowser/0.1 (+https://github.com/xandreeddev/desktop-lab)'

def allowed(url):
    value=urllib.parse.urlsplit(url)
    host=value.hostname or ''
    return value.scheme=='https' and not value.username and not value.password and value.port in (None,443) and (
        host in ('wallhaven.cc','th.wallhaven.cc','w.wallhaven.cc','alphacoders.com','wall.alphacoders.com') or
        re.fullmatch(r'images[0-9]*\.alphacoders\.com',host))

class Redirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self,req,fp,code,msg,headers,newurl):
        if not allowed(newurl): raise ValueError('Provider redirected outside its image hosts')
        return super().redirect_request(req,fp,code,msg,headers,newurl)

def fetch(url,limit=2*1024*1024):
    if not allowed(url): raise ValueError('Unsupported provider URL')
    started=time.monotonic()
    try:
        with urllib.request.build_opener(Redirects()).open(urllib.request.Request(url,headers={'User-Agent':AGENT}),timeout=8) as response:
            chunks=[];size=0
            while chunk:=response.read(min(65536,limit+1-size)):
                chunks.append(chunk);size+=len(chunk)
                if size>limit: raise ValueError('Provider response exceeds the download limit')
                if time.monotonic()-started>20: raise ValueError('Provider download timed out')
            return b''.join(chunks)
    except urllib.error.HTTPError as error:
        if error.code==429: raise ValueError('Provider rate limit reached. Try again later.') from None
        raise ValueError(f'Provider returned HTTP {error.code}. Try again later.') from None

def wallhaven(data,page):
    items=[]
    for entry in data.get('data',[])[:24]:
        if entry.get('purity')!='sfw': continue
        identity=str(entry['id'])
        if not re.fullmatch(r'[a-z0-9]{6}',identity): continue
        item={'provider':'wallhaven','id':identity,'title':'Wallhaven '+identity,
              'page_url':'https://wallhaven.cc/w/'+identity,'thumbnail_url':entry['thumbs']['large'],
              'image_url':entry['path'],'resolution':str(entry.get('resolution',''))}
        validate(item);items.append(item)
    return {'items':items,'page':page,'has_more':page<int(data.get('meta',{}).get('last_page',page))}

class AlphaResults(HTMLParser):
    """Read image cards/download metadata. Never evaluate provider JavaScript."""
    def __init__(self):
        super().__init__();self.items=[];self.current=None;self.canonical=None
    def handle_starttag(self,tag,attrs):
        values=dict(attrs)
        if tag=='link' and values.get('rel')=='canonical':
            url=values.get('href','')
            if allowed(url) and urllib.parse.urlsplit(url).hostname=='alphacoders.com':self.canonical=url
        if tag=='a' and (match:=re.fullmatch(r'https://wall\.alphacoders\.com/big\.php\?i=(\d+)',values.get('href',''))):
            self.current={'provider':'alpha-coders','id':match[1], 'page_url':values['href'],
                          'title':values.get('title','Alpha Coders '+match[1])[:180], 'resolution':'Original size',
                          'image_url':'','thumbnail_url':''}
        if self.current and tag=='img':
            source=values.get('data-src',values.get('src',''))
            if '/thumb-' in source and self.current['id'] in source:self.current['thumbnail_url']=source
        match=re.fullmatch(r"downloadContentModal\('(images\d*)',\s*(\d+),\s*'(jpg|jpeg|png|webp)',\s*\d+\); return false;",values.get('onclick',''))
        if self.current and match and match[2]==self.current['id'] and self.current['thumbnail_url']:
            host,identity,extension=match.groups()
            folder=urllib.parse.urlsplit(self.current['thumbnail_url']).path.split('/')[1]
            if not re.fullmatch(r'\d+',folder):raise ValueError('Invalid provider image folder')
            self.current['image_url']=f'https://{host}.alphacoders.com/{folder}/{identity}.{extension}'
            validate(self.current)
            if not any(item['id']==identity for item in self.items):self.items.append(self.current)
            self.current=None

def alpha(html,page):
    parser=AlphaResults();parser.feed(html)
    if not parser.items and ('big.php?i=' in html or 'Just a moment' in html or 'captcha' in html.lower()):
        raise ValueError('Alpha Coders could not be read. Its page format or availability may have changed.')
    # Site pagination is public HTML. An empty page is terminal; no fabricated API.
    return {'items':parser.items[:30],'page':page,'has_more':bool(parser.items)}

def validate(item):
    provider=item.get('provider');identity=item.get('id','')
    if provider not in ('wallhaven','alpha-coders') or not re.fullmatch(r'[a-zA-Z0-9]{1,16}',identity):raise ValueError('Invalid wallpaper identity')
    for key in ('image_url','thumbnail_url','page_url'):
        if not allowed(item.get(key,'')):raise ValueError('Invalid wallpaper URL')

def search(provider,query,page):
    if not 1<=page<=1000 or len(query)>200:raise ValueError('Invalid search or page')
    if provider=='wallhaven':
        url='https://wallhaven.cc/api/v1/search?'+urllib.parse.urlencode({'q':query,'page':page,'purity':'100','categories':'111','sorting':'relevance' if query else 'toplist'})
        return wallhaven(json.loads(fetch(url)),page)
    if provider=='alpha-coders':
        url='https://alphacoders.com/search/view?'+urllib.parse.urlencode({'q':query or 'landscape'})
        html=fetch(url).decode('utf-8')
        if page>1:
            parser=AlphaResults();parser.feed(html)
            target=urllib.parse.urlsplit(parser.canonical or url)
            params=[(key,value) for key,value in urllib.parse.parse_qsl(target.query) if key!='page']
            params.append(('page',str(page)))
            url=urllib.parse.urlunsplit((target.scheme,target.netloc,target.path,urllib.parse.urlencode(params),''))
            html=fetch(url).decode('utf-8')
        return alpha(html,page)
    raise ValueError('Unknown wallpaper provider')

def image_extension(data):
    if data.startswith(b'\x89PNG\r\n\x1a\n'):return 'png'
    if data.startswith(b'\xff\xd8\xff'):return 'jpg'
    if data.startswith(b'RIFF') and data[8:12]==b'WEBP':return 'webp'
    raise ValueError('Provider did not return a supported image')

def download(item,preview):
    validate(item)
    url=item['thumbnail_url' if preview else 'image_url']
    directory=CACHE if preview else LIBRARY
    directory.mkdir(parents=True,exist_ok=True)
    name=f"{item['provider']}-{item['id']}"+('-'+hashlib.sha256(url.encode()).hexdigest()[:12] if preview else '')
    for extension in ('png','jpg','webp'):
        path=directory/(name+'.'+extension)
        if path.is_file():return {'path':str(path),'name':item['title']}
    data=fetch(url,4*1024*1024 if preview else 32*1024*1024)
    path=directory/(name+'.'+image_extension(data))
    with tempfile.NamedTemporaryFile(dir=directory,delete=False) as stream:
        temporary=Path(stream.name);stream.write(data)
    temporary.replace(path)
    if not preview:
        path.with_suffix('.source.json').write_text(json.dumps({'provider':item['provider'],'page_url':item['page_url']},indent=2)+'\n')
    return {'path':str(path),'name':item['title']}

if __name__=='__main__':
    try:
        action,*args=sys.argv[1:]
        if action=='search' and len(args)==3:result=search(args[0],args[1],int(args[2]))
        elif action in ('preview','download') and len(args)==1:result=download(json.loads(args[0]),action=='preview')
        else:raise ValueError('Use search PROVIDER QUERY PAGE or preview/download ITEM_JSON')
        print(json.dumps(result))
    except (OSError,ValueError,KeyError,urllib.error.URLError) as error:
        # An error envelope crosses the process boundary without losing the provider message.
        print(json.dumps({'error':str(error)[:500]}))
