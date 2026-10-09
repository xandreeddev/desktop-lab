export const layers = [
  { id: 'client', number: '01', title: 'Desktop client', subtitle: 'The design lives here', tag: 'lucent-desktop', heading: 'One client. A whole desktop.', text: 'The client owns the bar, dock, launcher, selectors, widget state and visual identity. An independent hello-layer counter uses the same framework without importing any desktop code.', concepts: ['Bar & dock', 'Launcher', 'Seven widgets', 'Theme & layout'], flow: 'User input → typed message → component update → new view', source: 'lucent/apps/lucent-desktop/src/main.rs' },
  { id: 'domain', number: '02', title: 'Domain & use cases', subtitle: 'What the desktop means', tag: 'domain · usecases', heading: 'Behavior you can test without a screen.', text: 'Applications, workspaces, windows, settings and service snapshots are pure Rust models. Use cases rank searches, choose launch versus focus, clamp widget positions and validate configuration. Ports describe what an external service must do.', concepts: ['ApplicationId', 'Workspace', 'Widget placement', 'Service ports'], flow: 'Application query → ranked matches → launch-or-focus decision', source: 'lucent/crates/lucent-usecases/src/lib.rs' },
  { id: 'api', number: '03', title: 'Framework API', subtitle: 'Compose intent, not handles', tag: 'lucent-api', heading: 'Small concepts that fit together.', text: 'Components render declarative elements and update through typed messages. Effects carry work off the UI thread. Subscriptions deliver ongoing updates. SurfaceSpec declares native surfaces; Motion describes transitions. The API is independent of the desktop domain.', concepts: ['Component & Element', 'Effects & Subscription', 'SurfaceSpec', 'Motion & WidgetRegistry'], flow: 'child.view(cx).map(Message::Child) → composed element tree', source: 'lucent/crates/lucent-api/src/lib.rs' },
  { id: 'runtime', number: '04', title: 'Layout & Wayland runtime', subtitle: 'Input, surfaces, scheduling', tag: 'ui · wayland', heading: 'The bridge to a real desktop session.', text: 'The UI crate resolves layout, hit testing, focus and drag gestures. The SCTK runtime owns layer surfaces and keyboard/pointer input, executes effects and manages subscriptions. Compositor frame callbacks schedule only the surfaces that need a redraw.', concepts: ['Layout constraints', 'Native input regions', 'Effect worker', 'Frame callbacks'], flow: 'Dirty surface → compositor callback → layout → draw', source: 'lucent/crates/lucent-wayland/src/lib.rs' },
  { id: 'render', number: '05', title: 'Renderer & OS adapters', subtitle: 'Pixels and the outside world', tag: 'render · services', heading: 'Shared Vulkan. Explicit OS boundaries.', text: 'The renderer shares one wgpu Vulkan device and cached text/image resources across surfaces. Separate adapters talk to XDG applications, Hyprland IPC and system tools. OS operations stay out of component views and the draw loop.', concepts: ['wgpu / Vulkan', 'Cached text & images', 'Hyprland events', 'XDG / system adapters'], flow: 'Service event → typed snapshot → message → targeted redraw', source: 'lucent/crates/lucent-services/src/lib.rs' },
];

export const componentCode = `use lucent_api::*;

#[derive(Clone)]
enum Message { Increment }
struct Counter(u32);

impl Component for Counter {
    type Message = Message;

    fn view(&self, _: &ViewContext) -> Element<Message> {
        Element::row(vec![
            Element::text(format!("Count: {}", self.0)),
            Element::button("Add", Message::Increment)
                .id("increment"),
        ]).gap(12.).padding(16.)
    }

    fn update(&mut self, _: Message, fx: &mut Effects<Message>) {
        self.0 += 1;
        fx.redraw("example");
    }
}`;
