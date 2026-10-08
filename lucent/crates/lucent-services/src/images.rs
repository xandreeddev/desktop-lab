use lucent_api::ImageData;
use std::{fs,path::{Path,PathBuf},sync::Arc};

pub fn load(path:&Path,size:u32)->Option<Arc<ImageData>>{
    let bytes=fs::read(path).ok()?;
    if bytes.len()>32*1024*1024{return None;}
    let (width,height,rgba)=if path.extension().is_some_and(|s|s=="svg"){
        let options=resvg::usvg::Options{resources_dir:path.parent().map(Path::to_path_buf),..Default::default()};
        let tree=resvg::usvg::Tree::from_data(&bytes,&options).ok()?;
        let scale=size as f32/tree.size().width().max(tree.size().height());
        let width=(tree.size().width()*scale).ceil() as u32;let height=(tree.size().height()*scale).ceil() as u32;
        let mut pixmap=resvg::tiny_skia::Pixmap::new(width,height)?;
        resvg::render(&tree,resvg::tiny_skia::Transform::from_scale(scale,scale),&mut pixmap.as_mut());
        let mut pixels=pixmap.take();for px in pixels.chunks_exact_mut(4){if px[3]>0{for c in 0..3{px[c]=(u32::from(px[c])*255/u32::from(px[3])).min(255) as u8;}}}
        (width,height,pixels)
    }else{
        let mut reader=image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok()?;
        let mut limits=image::Limits::default();limits.max_image_width=Some(16384);limits.max_image_height=Some(16384);limits.max_alloc=Some(128*1024*1024);reader.limits(limits);
        let image=reader.decode().ok()?.thumbnail(size,size).to_rgba8();(image.width(),image.height(),image.into_raw())
    };
    Some(Arc::new(ImageData{key:format!("{}:{size}",path.display()),width,height,rgba}))
}
pub fn icon(name:&str)->Option<Arc<ImageData>>{
    if Path::new(name).is_absolute(){return load(Path::new(name),64);}
    let home=PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let roots=[home.join(".local/share/icons"),home.join(".icons"),PathBuf::from("/usr/share/icons")];
    for root in roots{
        for theme in ["Papirus-Dark","Papirus","Adwaita","hicolor"]{
            for size in ["64x64","48x48","scalable","32x32","symbolic"]{
                for category in ["apps","mimetypes","places","status"]{
                    for ext in ["png","svg"]{
                        let path=root.join(theme).join(size).join(category).join(format!("{name}.{ext}"));
                        if path.is_file()&&let Some(image)=load(&path,64){return Some(image);}
                    }
                }
            }
        }
    }
    for ext in ["png","svg"]{if let Some(image)=load(&PathBuf::from("/usr/share/pixmaps").join(format!("{name}.{ext}")),64){return Some(image);}}
    None
}
/// Small original line icons for framework clients; application logos come from XDG themes.
pub fn symbol(name:&str,color:&str)->Arc<ImageData>{
    let shape=match name{
        "apps"=>"<path d='M5 5h2v2H5zm6 0h2v2h-2zm6 0h2v2h-2zM5 11h2v2H5zm6 0h2v2h-2zm6 0h2v2h-2zM5 17h2v2H5zm6 0h2v2h-2zm6 0h2v2h-2z'/>",
        "search"=>"<circle cx='10.5' cy='10.5' r='6'/><path d='m15 15 5 5'/>",
        "wallpaper"=>"<rect x='3' y='4' width='18' height='16' rx='3'/><circle cx='8' cy='9' r='1'/><path d='m4 18 5-5 4 4 3-6 5 7'/>",
        "widgets"=>"<rect x='3' y='3' width='7' height='7' rx='1'/><rect x='14' y='3' width='7' height='7' rx='1'/><rect x='3' y='14' width='7' height='7' rx='1'/><rect x='14' y='14' width='7' height='7' rx='1'/>",
        "palette"=>"<path d='M12 3a9 9 0 1 0 0 18c4 0-2-5 2-5h3c6 0 5-13-5-13z'/><circle cx='7' cy='10' r='1'/><circle cx='11' cy='7' r='1'/><circle cx='16' cy='9' r='1'/>",
        "power"=>"<path d='M12 2v10M6 5a9 9 0 1 0 12 0'/>",
        "close"=>"<path d='m6 6 12 12M6 18 18 6'/>",
        "left"=>"<path d='m15 5-7 7 7 7'/>","right"=>"<path d='m9 5 7 7-7 7'/>",
        "music"=>"<path d='M9 17V5l11-2v12M9 7l11-2'/><ellipse cx='6' cy='18' rx='3' ry='2'/><ellipse cx='17' cy='16' rx='3' ry='2'/>",
        "play"=>"<path d='m8 4 12 8-12 8z'/>","pause"=>"<path d='M8 5v14M16 5v14' stroke-width='4'/>",
        "next"=>"<path d='m5 5 10 7-10 7zM19 5v14'/>","previous"=>"<path d='m19 5-10 7 10 7zM5 5v14'/>",
        "volume"=>"<path d='M3 9h4l5-5v16l-5-5H3zM16 8a6 6 0 0 1 0 8m3-11a10 10 0 0 1 0 14'/>",
        "network"=>"<path d='M3 8a14 14 0 0 1 18 0M6 12a9 9 0 0 1 12 0m-9 4a4 4 0 0 1 6 0'/><circle cx='12' cy='20' r='1'/>",
        "sun"=>"<circle cx='12' cy='12' r='4'/><path d='M12 1v3m0 16v3M1 12h3m16 0h3M4 4l2 2m12 12 2 2M4 20l2-2M18 6l2-2'/>",
        "cloud"=>"<path d='M6 19a5 5 0 1 1 0-10 7 7 0 0 1 13-1 5.5 5.5 0 0 1-1 11z'/>",
        "lock"=>"<rect x='5' y='10' width='14' height='11' rx='3'/><path d='M8 10V6a4 4 0 0 1 8 0v4M12 14v3'/>",
        "command"=>"<path d='M8 8h8v8H8zM8 8H5a3 3 0 1 1 3-3v3m8 0V5a3 3 0 1 1 3 3h-3m0 8h3a3 3 0 1 1-3 3v-3m-8 0v3a3 3 0 1 1-3-3h3'/>",
        _=>"<circle cx='12' cy='12' r='8'/><path d='M12 7v10M7 12h10'/>",
    };
    let svg=format!("<svg xmlns='http://www.w3.org/2000/svg' width='32' height='32' viewBox='0 0 24 24'><g fill='none' stroke='{color}' stroke-width='1.6' stroke-linecap='round' stroke-linejoin='round'>{shape}</g></svg>");
    let tree=resvg::usvg::Tree::from_str(&svg,&Default::default()).unwrap();let mut pixmap=resvg::tiny_skia::Pixmap::new(32,32).unwrap();resvg::render(&tree,resvg::tiny_skia::Transform::identity(),&mut pixmap.as_mut());
    let mut rgba=pixmap.take();for p in rgba.chunks_exact_mut(4){if p[3]>0{for c in 0..3{p[c]=(u32::from(p[c])*255/u32::from(p[3])).min(255) as u8;}}}
    Arc::new(ImageData{key:format!("symbol:{name}:{color}"),width:32,height:32,rgba})
}
