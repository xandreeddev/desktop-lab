@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var filtering: sampler;
@group(1) @binding(0) var<uniform> viewport: vec4<f32>;
struct ClipShape { rect: vec4<f32>, params: vec4<f32> };
@group(1) @binding(1) var<storage,read> clips: array<ClipShape>;
struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) rect: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) params: vec4<f32>,
    @location(4) clip: vec4<f32>,
    @location(5) uv: vec4<f32>,
    @location(6) @interpolate(flat) clip_range: vec2<u32>,
};
@vertex fn vs(@builtin(vertex_index) index:u32,@location(0) rect:vec4<f32>,@location(1) color:vec4<f32>,@location(2) params:vec4<f32>,@location(3) clip:vec4<f32>,@location(4) uv:vec4<f32>,@location(5) clip_range:vec4<f32>)->Vertex {
    let corners=array<vec2<f32>,6>(vec2(0.0,0.0),vec2(1.0,0.0),vec2(0.0,1.0),vec2(0.0,1.0),vec2(1.0,0.0),vec2(1.0,1.0));
    let pad=select(0.0,params.z*3.0,params.y>1.5&&params.y<2.5);
    let local=corners[index]*(rect.zw+vec2(pad*2.0))-vec2(pad);
    let point=rect.xy+local;
    var out:Vertex;
    out.position=vec4(point.x/viewport.x*2.0-1.0,1.0-point.y/viewport.y*2.0,0.0,1.0);
    out.local=local;out.rect=rect;out.color=color;out.params=params;out.clip=clip;
    out.uv=uv;out.clip_range=vec2<u32>(clip_range.xy);return out;
}
fn round_distance(point:vec2<f32>,rect:vec4<f32>,requested_radius:f32)->f32 {
    let radius=min(requested_radius,min(rect.z,rect.w)*0.5);
    let q=abs(point-rect.xy-rect.zw*0.5)-rect.zw*0.5+vec2(radius);
    return length(max(q,vec2(0.0)))+min(max(q.x,q.y),0.0)-radius;
}
@fragment fn fs(v:Vertex)->@location(0) vec4<f32> {
    let point=v.local+v.rect.xy;
    if any(point<v.clip.xy)||any(point>v.clip.xy+v.clip.zw){discard;}
    let d=round_distance(v.local,vec4(0.0,0.0,v.rect.zw),v.params.x);
    let aa=max(fwidth(d),0.3);
    var coverage=1.0-smoothstep(-aa,aa,d);
    // Axis-aligned texture quads already have exact rasterized bounds. Fading
    // their straight edges creates seams on full-surface wallpaper images.
    if v.params.y>0.5&&v.params.y<1.5&&v.params.x<=0.0 {coverage=1.0;}
    if v.params.y>1.5&&v.params.y<2.5&&v.params.z>0.0 {coverage=exp(-max(d,0.0)*max(d,0.0)/(v.params.z*v.params.z));}
    if v.params.y>2.5 {coverage*=smoothstep(-aa,aa,d+v.params.z);}
    var clip_distance=-1e6;
    for(var i=0u;i<v.clip_range.y;i+=1u) {
        let shape=clips[v.clip_range.x+i];
        clip_distance=max(clip_distance,round_distance(point,shape.rect,shape.params.x));
    }
    let clip_aa=max(fwidth(clip_distance),0.3);
    coverage=min(coverage,1.0-smoothstep(-clip_aa,clip_aa,clip_distance));
    var pixel=vec4(1.0);
    if v.params.y>0.5&&v.params.y<1.5 {pixel=textureSample(image,filtering,v.uv.xy+(v.local/v.rect.zw)*v.uv.zw);}
    return vec4(pixel.rgb*v.color.rgb*v.color.a,pixel.a*v.color.a)*coverage;
}
