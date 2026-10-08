struct Card {
    viewport: vec4<f32>, // physical width, height, output scale
    rect: vec4<f32>,     // logical x, y, width, height
    effects: vec4<f32>,  // animation scale, opacity, hover, palette blend
};
@group(0) @binding(0) var lettering: texture_2d<f32>;
@group(0) @binding(1) var filtering: sampler;
@group(0) @binding(2) var<uniform> card: Card;

@vertex fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corners = array<vec2<f32>, 6>(vec2(-1.0,-1.0), vec2(1.0,-1.0), vec2(-1.0,1.0),
                                     vec2(-1.0,1.0), vec2(1.0,-1.0), vec2(1.0,1.0));
    let center = card.rect.xy + card.rect.zw * 0.5;
    let extent = (card.rect.zw * 0.5 + vec2(40.0)) * card.effects.x;
    let physical = (center + corners[index] * extent) * card.viewport.z;
    return vec4(physical.x / card.viewport.x * 2.0 - 1.0,
                1.0 - physical.y / card.viewport.y * 2.0, 0.0, 1.0);
}
fn rounded(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half_size + vec2(radius);
    return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}
fn over(front: vec4<f32>, back: vec4<f32>) -> vec4<f32> {
    return front + back * (1.0 - front.a);
}
@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let half_size = card.rect.zw * 0.5;
    let local = (p.xy / card.viewport.z - card.rect.xy - half_size) / card.effects.x;
    let uv = (local + half_size) / card.rect.zw;
    let distance = rounded(local, half_size, 26.0);
    let aa = max(fwidth(distance), 0.25);
    let coverage = 1.0 - smoothstep(-aa, aa, distance);
    let shadow_d = max(rounded(local - vec2(0.0, 8.0), half_size, 26.0), 0.0);
    let shadow = exp(-shadow_d * shadow_d / 180.0) * 0.28;
    let blue = mix(vec3(0.082, 0.119, 0.175), vec3(0.12, 0.185, 0.25), clamp(1.0-uv.y, 0.0, 1.0));
    let teal = mix(vec3(0.068, 0.15, 0.15), vec3(0.105, 0.23, 0.22), clamp(1.0-uv.y, 0.0, 1.0));
    let base = mix(blue, teal, card.effects.w) + card.effects.z * vec3(0.014, 0.02, 0.025);
    let alpha = coverage * 0.97;
    var result = over(vec4(base * alpha, alpha), vec4(0.0, 0.0, 0.0, shadow));
    let border = (1.0 - smoothstep(0.4, 1.6, abs(distance + 0.7))) * coverage;
    let border_alpha = border * (0.16 + 0.18 * card.effects.z);
    result = over(vec4(vec3(0.56, 0.8, 0.9) * border_alpha, border_alpha), result);
    let dot = (1.0 - smoothstep(3.8, 4.6, length(local + half_size - vec2(35.0, 35.0)))) * coverage;
    result = over(vec4(vec3(0.5, 0.84, 0.87) * dot, dot), result);
    // Six dots form a small grip in the upper-right corner.
    let grip = local + half_size - vec2(491.0, 30.0);
    let cell = abs(grip - clamp(round(grip / 7.0), vec2(0.0), vec2(1.0, 2.0)) * 7.0);
    let grip_alpha = (1.0 - smoothstep(1.0, 1.8, length(cell))) * (0.35 + card.effects.z * 0.35);
    result = over(vec4(vec3(0.7, 0.84, 0.9) * grip_alpha, grip_alpha), result);
    let separator = (1.0-smoothstep(0.0, 0.8, abs(local.y + half_size.y - 158.0)))
        * step(30.0, local.x + half_size.x) * step(local.x + half_size.x, card.rect.z - 30.0) * 0.1;
    result = over(vec4(vec3(0.7, 0.84, 0.9) * separator, separator), result);
    let text = textureSample(lettering, filtering, uv) * coverage;
    return over(text, result) * card.effects.y;
}
