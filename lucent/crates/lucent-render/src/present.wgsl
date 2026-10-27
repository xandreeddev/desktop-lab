@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var filtering: sampler;
fn sample_color(uv:vec2<f32>)->vec4<f32> {return textureSample(image,filtering,uv);}
