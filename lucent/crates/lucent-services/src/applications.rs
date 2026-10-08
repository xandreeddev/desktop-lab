//! XDG desktop entry discovery and direct argv launching.
use lucent_domain::*;
use std::{collections::BTreeMap,fs,os::unix::fs::PermissionsExt,path::{Path,PathBuf},process::{Command,Stdio}};

#[derive(Default)]pub struct XdgApplications;
impl ApplicationPort for XdgApplications{
    fn discover(&self)->Result<Vec<Application>>{
        let home=std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let data=std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p|p.is_absolute()).unwrap_or_else(||home.join(".local/share"));
        let mut roots=vec![data];
        roots.extend(std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_|"/usr/local/share:/usr/share".into()).split(':').filter(|s|s.starts_with('/')).map(PathBuf::from));
        let desktops=std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_|"Hyprland".into());
        let locale=std::env::var("LC_MESSAGES").or_else(|_|std::env::var("LANG")).unwrap_or_default();
        let mut entries=BTreeMap::new();
        for root in roots{
            let root=root.join("applications");let mut files=vec![];walk(&root,0,&mut files);files.sort();
            for path in files{
                let id=path.strip_prefix(&root).unwrap().to_string_lossy().replace('/',"-");
                if entries.contains_key(&id){continue;}
                // Hidden and invalid entries still mask lower-priority entries with the same ID.
                let entry=fs::read_to_string(&path).ok().and_then(|text|parse_entry(&id,&path.to_string_lossy(),&text,&desktops,&locale).ok().flatten());
                entries.insert(id,entry);
            }
        }
        Ok(entries.into_values().flatten().collect())
    }
    fn launch(&self,app:&Application)->Result<()>{
        let command=&app.command;
        let mut process=if command.terminal{
            let terminal=["foot","alacritty","kitty"].into_iter().find(|p|executable(p)).ok_or_else(||DomainError::Unavailable("No supported terminal is installed".into()))?;
            let mut c=Command::new(terminal);c.arg("-e").arg(&command.program);c
        }else{Command::new(&command.program)};
        process.args(&command.args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        if let Some(path)=&command.directory{process.current_dir(path);}
        let mut child=process.spawn().map_err(|e|DomainError::Failed(format!("Cannot launch {}: {e}",app.name)))?;
        std::thread::spawn(move||{let _=child.wait();});Ok(())
    }
}
fn walk(path:&Path,depth:u8,out:&mut Vec<PathBuf>){
    if depth>8{return;}
    if let Ok(dir)=fs::read_dir(path){for entry in dir.flatten(){
        let p=entry.path();if p.is_dir() && !entry.file_type().is_ok_and(|t|t.is_symlink()){walk(&p,depth+1,out);}
        else if p.extension().is_some_and(|s|s=="desktop"){out.push(p);}
    }}
}
fn executable(program:&str)->bool{
    let check=|p:&Path|fs::metadata(p).is_ok_and(|m|m.is_file()&&m.permissions().mode()&0o111!=0);
    if program.contains('/') {return check(Path::new(program));}
    std::env::var_os("PATH").is_some_and(|p|std::env::split_paths(&p).any(|d|check(&d.join(program))))
}
fn unescape(value:&str)->String{
    let mut out=String::new();let mut chars=value.chars();
    while let Some(c)=chars.next(){if c=='\\'{match chars.next(){Some('s')=>out.push(' '),Some('n')=>out.push('\n'),Some('t')=>out.push('\t'),Some('r')=>out.push('\r'),Some('\\')=>out.push('\\'),Some(c)=>{out.push('\\');out.push(c)},None=>out.push('\\')}}else{out.push(c)}}out
}
pub fn parse_entry(id:&str,path:&str,text:&str,desktops:&str,locale:&str)->Result<Option<Application>>{
    let mut main=false;let mut values=BTreeMap::new();
    for line in text.lines(){let line=line.trim();if line.starts_with('['){main=line=="[Desktop Entry]";continue;}if main&&!line.starts_with('#')&&let Some((k,v))=line.split_once('='){values.insert(k.trim(),v.trim());}}
    let value=|k:&str|values.get(k).copied().unwrap_or("");
    if value("Type")!="Application"||value("Hidden")=="true"||value("NoDisplay")=="true"{return Ok(None);}
    let desktop_names:Vec<_>=desktops.split(':').collect();
    if !value("OnlyShowIn").is_empty()&&!value("OnlyShowIn").split(';').any(|d|desktop_names.contains(&d)){return Ok(None);}
    if value("NotShowIn").split(';').filter(|s|!s.is_empty()).any(|d|desktop_names.contains(&d)){return Ok(None);}
    if !value("TryExec").is_empty()&&!executable(&unescape(value("TryExec"))){return Ok(None);}
    let locale=locale.split('.').next().unwrap_or(locale);let language=locale.split('_').next().unwrap_or(locale);
    let localized=|key:&str|->String{for l in [locale,language]{if let Some(v)=values.get(format!("{key}[{l}]").as_str()){return unescape(v);}}unescape(value(key))};
    let name=localized("Name");if name.is_empty()||value("Exec").is_empty(){return Ok(None);}
    let icon=unescape(value("Icon"));
    let argv=parse_exec(&unescape(value("Exec")),&name,path,&icon)?;
    let directory=unescape(value("Path"));
    Ok(Some(Application{id:AppId(id.into()),name,description:localized("Comment"),keywords:localized("Keywords").split(';').map(str::to_owned).collect(),icon,startup_class:unescape(value("StartupWMClass")),command:LaunchCommand{program:argv[0].clone(),args:argv[1..].into(),directory:(!directory.is_empty()).then_some(directory),terminal:value("Terminal")=="true"}}))
}
/// Parse the desktop-entry quoting grammar, then expand each field code once.
pub fn parse_exec(exec:&str,name:&str,path:&str,icon:&str)->Result<Vec<String>>{
    let invalid=||DomainError::Invalid("Invalid desktop entry Exec field".into());
    let mut tokens=vec![];let mut token=String::new();let mut quoted=false;let mut started=false;let mut chars=exec.chars();
    while let Some(c)=chars.next(){match c{
        '"'=>{quoted=!quoted;started=true;},
        '\\'=>{let c=chars.next().ok_or_else(invalid)?;token.push(c);started=true;},
        c if c.is_whitespace()&&!quoted=>{if started{tokens.push(std::mem::take(&mut token));started=false;}},
        c=>{token.push(c);started=true;},
    }}
    if quoted{return Err(invalid());}if started{tokens.push(token);}
    let mut out=vec![];
    for token in tokens{
        match token.as_str(){"%f"|"%F"|"%u"|"%U"|"%d"|"%D"|"%n"|"%N"|"%v"|"%m"=>continue,
            "%i"=>{if !icon.is_empty(){out.extend(["--icon".into(),icon.into()]);}continue;},_=>{}}
        let mut expanded=String::new();let mut cs=token.chars();
        while let Some(c)=cs.next(){if c!='%'{expanded.push(c);continue;}match cs.next(){
            Some('%')=>expanded.push('%'),Some('c')=>expanded.push_str(name),Some('k')=>expanded.push_str(path),
            Some('f'|'u'|'d'|'D'|'n'|'N'|'v'|'m')=>{},_=>return Err(invalid()),
        }}out.push(expanded);
    }
    if out.first().is_none_or(|s|s.is_empty()||s.contains('=')){return Err(invalid());}Ok(out)
}

#[cfg(test)]mod tests{use super::*;
    #[test]fn argv_preserves_quoting_and_does_not_interpret_shell_text(){assert_eq!(parse_exec(r#"app "two words" "$(touch /tmp/no)" %% %c %k %i %U"#,"App %U","/a b.desktop","icon").unwrap(),vec!["app","two words","$(touch /tmp/no)","%","App %U","/a b.desktop","--icon","icon"]);}
    #[test]fn rejects_unknown_codes_and_unbalanced_quotes(){for s in ["app %x","app %i-suffix","app \"broken"]{assert!(parse_exec(s,"","","").is_err());}}
    #[test]fn hidden_and_foreign_desktop_entries_are_omitted(){for extra in ["Hidden=true","NoDisplay=true","OnlyShowIn=GNOME;","NotShowIn=Hyprland;"]{let text=format!("[Desktop Entry]\nType=Application\nName=Test\nExec=test\n{extra}");assert!(parse_entry("test.desktop","/test.desktop",&text,"Hyprland","en_US").unwrap().is_none());}}
    #[test]fn desktop_actions_do_not_replace_main_exec(){let app=parse_entry("a.desktop","/a.desktop","[Desktop Entry]\nType=Application\nName=Test\nName[sv]=Prov\nExec=app %U\n[Desktop Action Other]\nExec=other","Hyprland","sv_SE.UTF-8").unwrap().unwrap();assert_eq!(app.name,"Prov");assert_eq!(app.command.program,"app");}
}
