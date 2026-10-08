//! A small declarative shell API. Public types contain no Wayland or GPU handles.
use std::{collections::BTreeSet, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Duration};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect { pub x:f32, pub y:f32, pub w:f32, pub h:f32 }
impl Rect {
    pub fn new(x:f32,y:f32,w:f32,h:f32)->Self { Self{x,y,w:w.max(0.),h:h.max(0.)} }
    pub fn contains(self,x:f32,y:f32)->bool { x>=self.x && y>=self.y && x<self.x+self.w && y<self.y+self.h }
    pub fn intersect(self,other:Self)->Self {
        let x=self.x.max(other.x); let y=self.y.max(other.y);
        Self::new(x,y,(self.x+self.w).min(other.x+other.w)-x,(self.y+self.h).min(other.y+other.h)-y)
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color(pub f32,pub f32,pub f32,pub f32);
impl Color {
    pub const TRANSPARENT:Self=Self(0.,0.,0.,0.);
    pub fn hex(rgb:u32)->Self {Self(((rgb>>16)&255) as f32/255.,((rgb>>8)&255) as f32/255.,(rgb&255) as f32/255.,1.)}
    pub fn alpha(self,a:f32)->Self {Self(self.0,self.1,self.2,a)}
    pub fn mix(self,b:Self,t:f32)->Self { Self(self.0+(b.0-self.0)*t,self.1+(b.1-self.1)*t,self.2+(b.2-self.2)*t,self.3+(b.3-self.3)*t) }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Length { Fixed(f32), Fill, #[default] Shrink }
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Align { #[default] Start, Center, End }
#[derive(Clone, Debug)]
pub struct Style {
    pub width:Length, pub height:Length, pub padding:f32, pub gap:f32,
    pub position:Option<(f32,f32)>, pub background:Color, pub foreground:Color,
    pub radius:f32, pub font_size:f32, pub align:Align, pub opacity:f32,
    pub clip:bool, pub shadow:bool,
}
impl Default for Style {
    fn default()->Self { Self {width:Length::Shrink,height:Length::Shrink,padding:0.,gap:0.,position:None,
        background:Color::TRANSPARENT,foreground:Color::hex(0xc2e8f5),radius:0.,font_size:14.,align:Align::Start,
        opacity:1.,clip:false,shadow:false} }
}
#[derive(Clone, Debug)]
pub struct ImageData { pub key:String,pub width:u32,pub height:u32,pub rgba:Vec<u8> }
#[derive(Clone, Debug)]
pub enum Kind { Row,Column,Stack,Grid(usize),Text(String),Image(Arc<ImageData>),Input{value:String,placeholder:String},Empty }
#[derive(Clone, Copy, Debug)]
pub struct DragEvent {pub dx:f32,pub dy:f32,pub finished:bool}
pub type InputCallback<M> = Arc<dyn Fn(String)->M+Send+Sync>;
pub type DragCallback<M> = Arc<dyn Fn(DragEvent)->M+Send+Sync>;
#[derive(Clone)]
pub struct Element<M> {
    pub id:String,pub kind:Kind,pub style:Style,pub children:Vec<Element<M>>,
    pub click:Option<M>,pub hover:Option<M>,pub input:Option<InputCallback<M>>,
    pub drag:Option<DragCallback<M>>,pub autofocus:bool,
}
impl<M> Element<M> {
    pub fn new(kind:Kind)->Self { Self{id:String::new(),kind,style:Style::default(),children:vec![],click:None,hover:None,input:None,drag:None,autofocus:false} }
    pub fn text(text:impl Into<String>)->Self {Self::new(Kind::Text(text.into()))}
    pub fn image(image:Arc<ImageData>)->Self {Self::new(Kind::Image(image))}
    pub fn row(children:Vec<Self>)->Self {let mut e=Self::new(Kind::Row);e.children=children;e}
    pub fn column(children:Vec<Self>)->Self {let mut e=Self::new(Kind::Column);e.children=children;e}
    pub fn stack(children:Vec<Self>)->Self {let mut e=Self::new(Kind::Stack);e.children=children;e}
    pub fn grid(columns:usize,children:Vec<Self>)->Self {let mut e=Self::new(Kind::Grid(columns.max(1)));e.children=children;e}
    pub fn empty()->Self {Self::new(Kind::Empty)}
    pub fn button(label:impl Into<String>,message:M)->Self {Self::text(label).on_click(message).padding(10.).radius(20.)}
    pub fn input(value:impl Into<String>,placeholder:impl Into<String>,on_change:impl Fn(String)->M+Send+Sync+'static)->Self {
        let mut e=Self::new(Kind::Input{value:value.into(),placeholder:placeholder.into()});e.input=Some(Arc::new(on_change));e
    }
    pub fn id(mut self,id:impl Into<String>)->Self{self.id=id.into();self}
    pub fn width(mut self,v:Length)->Self{self.style.width=v;self}
    pub fn height(mut self,v:Length)->Self{self.style.height=v;self}
    pub fn size(self,w:f32,h:f32)->Self{self.width(Length::Fixed(w)).height(Length::Fixed(h))}
    pub fn fill(self)->Self{self.width(Length::Fill).height(Length::Fill)}
    pub fn at(mut self,x:f32,y:f32)->Self{self.style.position=Some((x,y));self}
    pub fn padding(mut self,v:f32)->Self{self.style.padding=v;self}
    pub fn gap(mut self,v:f32)->Self{self.style.gap=v;self}
    pub fn background(mut self,v:Color)->Self{self.style.background=v;self}
    pub fn color(mut self,v:Color)->Self{self.style.foreground=v;self}
    pub fn radius(mut self,v:f32)->Self{self.style.radius=v;self}
    pub fn font(mut self,v:f32)->Self{self.style.font_size=v;self}
    pub fn align(mut self,v:Align)->Self{self.style.align=v;self}
    pub fn opacity(mut self,v:f32)->Self{self.style.opacity=v;self}
    pub fn clip(mut self)->Self{self.style.clip=true;self}
    pub fn shadow(mut self)->Self{self.style.shadow=true;self}
    pub fn on_click(mut self,msg:M)->Self{self.click=Some(msg);self}
    pub fn on_hover(mut self,msg:M)->Self{self.hover=Some(msg);self}
    pub fn on_drag(mut self,f:impl Fn(DragEvent)->M+Send+Sync+'static)->Self{self.drag=Some(Arc::new(f));self}
    pub fn autofocus(mut self)->Self{self.autofocus=true;self}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {Background,Bottom,Top,Overlay}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {Top,Bottom,Fill,Center}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyboard {None,OnDemand,Exclusive}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceSpec {
    pub id:&'static str,pub layer:Layer,pub anchor:Anchor,pub width:u32,pub height:u32,
    pub exclusive_zone:i32,pub keyboard:Keyboard,pub visible:bool,pub capture_all:bool,
}
#[derive(Clone, Debug)]
pub struct ViewContext {pub surface:&'static str,pub width:f32,pub height:f32,pub now:f64}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {Text(String),Backspace,Delete,Enter,Escape,Up,Down,Left,Right,Tab,Home,End,SelectAll}
#[derive(Clone, Debug)]
pub enum Event {Key{surface:&'static str,key:Key},Scroll{surface:&'static str,lines:f64},Outside{surface:&'static str}}

/// Components own their state; the runtime delivers typed messages on one UI thread.
pub trait Component:'static {
    type Message:Clone+Send+'static;
    fn view(&self,cx:&ViewContext)->Element<Self::Message>;
    fn update(&mut self,message:Self::Message,effects:&mut Effects<Self::Message>);
}
/// An application supplies surfaces and subscriptions in addition to its component tree.
pub trait Application:Component {
    fn name(&self)->&'static str;
    fn surfaces(&self)->Vec<SurfaceSpec>;
    fn init(&mut self,_effects:&mut Effects<Self::Message>){}
    fn subscriptions(&self)->Vec<Subscription<Self::Message>>{vec![]}
    fn event(&self,_event:Event)->Option<Self::Message>{None}
    fn command(&self,_command:&str)->Result<Option<Self::Message>,String>{Err("Unknown command".into())}
    fn inspect(&self)->String {"{}".into()}
    fn animating(&self,_surface:&str,_now:f64)->bool{false}
}

pub type Task<M> = Box<dyn FnOnce()->M+Send>;
pub struct Effects<M> {pub tasks:Vec<Task<M>>,pub redraw:BTreeSet<&'static str>,pub exit:bool}
impl<M> Default for Effects<M>{fn default()->Self{Self{tasks:vec![],redraw:BTreeSet::new(),exit:false}}}
impl<M> Effects<M> {
    pub fn task(&mut self,task:impl FnOnce()->M+Send+'static){self.tasks.push(Box::new(task));}
    pub fn redraw(&mut self,surface:&'static str){self.redraw.insert(surface);}
    pub fn quit(&mut self){self.exit=true;}
}
#[derive(Clone)]
pub struct Cancellation(Arc<AtomicBool>);
impl Default for Cancellation{fn default()->Self{Self(Arc::new(AtomicBool::new(false)))}}
impl Cancellation{
    pub fn cancel(&self){self.0.store(true,Ordering::Relaxed);}
    pub fn cancelled(&self)->bool{self.0.load(Ordering::Relaxed)}
    pub fn sleep(&self,duration:Duration){
        let start=std::time::Instant::now();
        while !self.cancelled() && start.elapsed()<duration {std::thread::sleep((duration-start.elapsed().min(duration)).min(Duration::from_millis(100)));}
    }
}
pub struct Emitter<M>(Arc<dyn Fn(M)+Send+Sync>);
impl<M> Clone for Emitter<M>{fn clone(&self)->Self{Self(self.0.clone())}}
impl<M> Emitter<M>{
    pub fn new(f:impl Fn(M)+Send+Sync+'static)->Self{Self(Arc::new(f))}
    pub fn send(&self,message:M){(self.0)(message)}
}
pub struct Subscription<M>{pub id:&'static str,pub run:Box<dyn FnOnce(Emitter<M>,Cancellation)+Send>}
impl<M:Send+'static> Subscription<M>{
    pub fn stream(id:&'static str,run:impl FnOnce(Emitter<M>,Cancellation)+Send+'static)->Self{Self{id,run:Box::new(run)}}
    pub fn every(id:&'static str,interval:Duration,make:impl Fn()->M+Send+'static)->Self{
        Self::stream(id,move|out,cancel|{while !cancel.cancelled(){out.send(make());cancel.sleep(interval);}})
    }
}

/// Interruptible cubic Bézier animation. X is time; Y may overshoot for spatial motion.
#[derive(Clone, Copy, Debug)]
pub struct Motion{from:f32,to:f32,start:f64,duration:f64,curve:[f32;4]}
impl Motion{
    pub fn fixed(value:f32)->Self{Self{from:value,to:value,start:0.,duration:0.3,curve:[0.05,0.7,0.1,1.]}}
    pub fn value(self,now:f64)->f32{
        if now>=self.start+self.duration{return self.to;}
        let x=((now-self.start)/self.duration).clamp(0.,1.) as f32;
        let bez=|t:f32,a:f32,b:f32|3.*(1.-t).powi(2)*t*a+3.*(1.-t)*t*t*b+t*t*t;
        let(mut lo,mut hi)=(0.,1.);
        for _ in 0..14{let m=(lo+hi)*0.5;if bez(m,self.curve[0],self.curve[2])<x{lo=m}else{hi=m}}
        self.from+(self.to-self.from)*bez((lo+hi)*0.5,self.curve[1],self.curve[3])
    }
    pub fn target(&mut self,value:f32,now:f64,duration:f64,curve:[f32;4]){
        if self.to!=value{self.from=self.value(now);self.to=value;self.start=now;self.duration=duration.max(0.001);self.curve=curve;}
    }
    pub fn active(self,now:f64)->bool{self.from!=self.to && now<self.start+self.duration}
    pub fn target_value(self)->f32{self.to}
}

#[cfg(test)]mod tests{use super::*;
    #[test]fn interrupted_motion_is_continuous_and_finishes(){let mut m=Motion::fixed(0.);m.target(1.,0.,0.5,[0.38,1.21,0.22,1.]);let v=m.value(0.2);m.target(0.,0.2,0.3,[0.05,0.7,0.1,1.]);assert!((m.value(0.2)-v).abs()<0.001);assert!(!m.active(1.));assert_eq!(m.value(1.),0.);}
    #[test]fn intersection_never_has_negative_extent(){assert_eq!(Rect::new(0.,0.,10.,10.).intersect(Rect::new(20.,20.,3.,3.)).w,0.);}
}
