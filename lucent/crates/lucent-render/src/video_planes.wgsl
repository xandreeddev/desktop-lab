// Explicit BT.709 matrix/primaries; known BT.709 or sRGB transfer only.
// Output is nonlinear sRGB for Bgra8Unorm, NOT an sRGB render attachment.
struct Metadata {extent: vec4<f32>, color: vec4<f32>};
@group(0) @binding(0) var y_plane: texture_2d<f32>;
@group(0) @binding(1) var u_plane: texture_2d<f32>;
@group(0) @binding(2) var v_plane: texture_2d<f32>;
@group(0) @binding(3) var<uniform> metadata: Metadata;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) index: u32) -> Vertex {
    let positions = array<vec2<f32>, 3>(vec2(-1.,1.), vec2(3.,1.), vec2(-1.,-3.));
    let position = positions[index];
    return Vertex(vec4(position,0.,1.), vec2((position.x+1.)*.5,(1.-position.y)*.5));
}
fn display_srgb(signal: vec3<f32>) -> vec3<f32> {
    if metadata.color.y == 0. {return signal;}
    // Invert BT.709 OETF, then encode sRGB. Matching primaries/white need no
    // chromatic-adaptation matrix; no hardware sRGB conversion follows this.
    let linear = select(pow((signal+vec3(.09929683))/1.0992968,vec3(1./.45)),signal/4.5,signal<vec3(.08124286));
    return select(1.055*pow(linear,vec3(1./2.4))-vec3(.055),12.92*linear,linear<=vec3(.0031308));
}
// Explicit float32 interpolation avoids depending on filtered R8 sampling's
// intermediate precision. Four loads/plane have a cost; no throughput claim.
// This remains one-level bilinear sampling, not an antialias/minification filter.
fn sample_plane(plane: texture_2d<f32>, at: vec2<f32>) -> f32 {
    let dimensions = vec2<i32>(textureDimensions(plane));
    let last = dimensions-vec2<i32>(1);
    let pixel = clamp(at,vec2(0.),vec2<f32>(last));
    let lower = vec2<i32>(floor(pixel));
    let upper = min(lower+vec2<i32>(1),last);
    let fraction = fract(pixel);
    let a = textureLoad(plane,lower,0).r;
    let b = textureLoad(plane,vec2(upper.x,lower.y),0).r;
    let c = textureLoad(plane,vec2(lower.x,upper.y),0).r;
    let d = textureLoad(plane,upper,0).r;
    return mix(mix(a,b,fraction.x),mix(c,d,fraction.x),fraction.y);
}
@fragment fn fs(vertex: Vertex) -> @location(0) vec4<f32> {
    // Work in luma sample coordinates. Ceil-sized chroma extents and siting
    // offsets matter at odd source edges and when scaling; plain UV reuse fails.
    let luma_index = vertex.uv*metadata.extent.xy-vec2(.5);
    let chroma_index = (luma_index-metadata.color.zw)*.5;
    let y_sample = sample_plane(y_plane,luma_index);
    let u_sample = sample_plane(u_plane,chroma_index);
    let v_sample = sample_plane(v_plane,chroma_index);
    var y = y_sample;
    var cb = (u_sample*255.-128.)/255.;
    var cr = (v_sample*255.-128.)/255.;
    if metadata.color.x != 0. {
        y = (y_sample*255.-16.)/219.;
        cb = (u_sample*255.-128.)/224.;
        cr = (v_sample*255.-128.)/224.;
    }
    let signal = clamp(vec3(y+1.5748*cr,y-.18732427*cb-.46812427*cr,y+1.8556*cb),vec3(0.),vec3(1.));
    return vec4(display_srgb(signal),1.);
}
