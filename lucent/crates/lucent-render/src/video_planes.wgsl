struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) index: u32) -> Vertex {
    let positions = array<vec2<f32>, 3>(vec2(-1.,1.), vec2(3.,1.), vec2(-1.,-3.));
    let position = positions[index];
    return Vertex(vec4(position,0.,1.), vec2((position.x+1.)*.5,(1.-position.y)*.5));
}
@fragment fn fs(vertex: Vertex) -> @location(0) vec4<f32> {return sample_color(vertex.uv);}
