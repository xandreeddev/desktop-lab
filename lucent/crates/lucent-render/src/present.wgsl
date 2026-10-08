@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var filtering: sampler;
@group(1) @binding(0) var<uniform> viewport: vec4<f32>;
struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) rect: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) params: vec4<f32>,
    @location(4) clip: vec4<f32>,
};
@vertex fn vs(@builtin(vertex_index) index:u32,@location(0) rect:vec4<f32>,@location(1) color:vec4<f32>,@location(2) params:vec4<f32>,@location(3) clip:vec4<f32>)->Vertex {
    let corners=array<vec2<f32>,6>(vec2(0.0,0.0),vec2(1.0,0.0),vec2(0.0,1.0),vec2(0.0,1.0),vec2(1.0,0.0),vec2(1.0,1.0));
    let pad=select(0.0,18.0,params.y>1.5);
    let local=corners[index]*(rect.zw+vec2(pad*2.0))-vec2(pad);
    let point=rect.xy+local;
    var out:Vertex;
    out.position=vec4(point.x/viewport.x*2.0-1.0,1.0-point.y/viewport.y*2.0,0.0,1.0);
    out.local=local;out.rect=rect;out.color=color;out.params=params;out.clip=clip;return out;
}
@fragment fn fs(v:Vertex)->@location(0) vec4<f32> {
    let point=v.local+v.rect.xy;
    if any(point<v.clip.xy)||any(point>v.clip.xy+v.clip.zw){discard;}
    let radius=min(v.params.x,min(v.rect.z,v.rect.w)*0.5);
    let q=abs(v.local-v.rect.zw*0.5)-v.rect.zw*0.5+vec2(radius);
    let d=length(max(q,vec2(0.0)))+min(max(q.x,q.y),0.0)-radius;
    let aa=max(fwidth(d),0.3);
    var coverage=1.0-smoothstep(-aa,aa,d);
    if v.params.y>1.5 {coverage=exp(-max(d,0.0)*max(d,0.0)/65.0);}
    var pixel=vec4(1.0);
    if v.params.y>0.5&&v.params.y<1.5 {pixel=textureSample(image,filtering,v.local/v.rect.zw);}
    return vec4(pixel.rgb*v.color.rgb*v.color.a,pixel.a*v.color.a)*coverage;
}
