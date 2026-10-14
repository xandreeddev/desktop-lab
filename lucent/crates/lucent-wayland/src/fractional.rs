//! Optional fractional-scale and viewport protocol adapters. No client policy.
use super::*;
use wayland_client::{Dispatch, globals::GlobalList};
use wayland_protocols::wp::{
    fractional_scale::v1::client::{
        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        wp_fractional_scale_v1::{self, WpFractionalScaleV1},
    },
    viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
};
pub(super) struct Globals {
    manager: WpFractionalScaleManagerV1,
    viewporter: WpViewporter,
}
impl Globals {
    pub fn bind<A: Application>(globals: &GlobalList, qh: &QueueHandle<State<A>>) -> Option<Self> {
        let manager: WpFractionalScaleManagerV1 = globals.bind(qh, 1..=1, ()).ok()?;
        let viewporter: WpViewporter = match globals.bind(qh, 1..=1, ()) {
            Ok(viewporter) => viewporter,
            Err(_) => {
                manager.destroy();
                return None;
            }
        };
        Some(Self {
            manager,
            viewporter,
        })
    }
    pub fn surface<A: Application>(
        &self,
        surface: &wl_surface::WlSurface,
        qh: &QueueHandle<State<A>>,
    ) -> Surface {
        Surface {
            scale: self
                .manager
                .get_fractional_scale(surface, qh, surface.clone()),
            viewport: self.viewporter.get_viewport(surface, qh, ()),
            preferred: None,
            destination: None,
        }
    }
}
impl Drop for Globals {
    fn drop(&mut self) {
        self.manager.destroy();
        self.viewporter.destroy();
    }
}
pub(super) struct Surface {
    scale: WpFractionalScaleV1,
    viewport: WpViewport,
    pub preferred: Option<api::Density>,
    destination: Option<(u32, u32)>,
}
impl Surface {
    pub fn destination(&mut self, width: u32, height: u32) -> Result<(), Error> {
        let width_i32 =
            i32::try_from(width).map_err(|_| "Viewport width exceeds protocol range")?;
        let height_i32 =
            i32::try_from(height).map_err(|_| "Viewport height exceeds protocol range")?;
        if width_i32 <= 0 || height_i32 <= 0 {
            return Err("Viewport dimensions must be positive".into());
        }
        if self.destination != Some((width, height)) {
            self.viewport.set_destination(width_i32, height_i32);
            self.destination = Some((width, height));
        }
        Ok(())
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        self.viewport.destroy();
        self.scale.destroy();
    }
}
impl<A: Application> Dispatch<WpFractionalScaleV1, wl_surface::WlSurface> for State<A> {
    fn event(
        state: &mut Self,
        proxy: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        surface: &wl_surface::WlSurface,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wp_fractional_scale_v1::Event::PreferredScale { scale } = event else {
            return;
        };
        let Ok(density) = api::Density::new(scale, 120) else {
            return;
        };
        let Some(id) = state.id_for(surface) else {
            return;
        };
        let native = state.surfaces.get_mut(&id).expect("current surface");
        let before = native.density();
        let Some(fractional) = native
            .fractional
            .as_mut()
            .filter(|value| &value.scale == proxy)
        else {
            return;
        };
        fractional.preferred = Some(density);
        if before == density {
            return;
        }
        native.demand.invalidate();
        if native.configured {
            let event = native.spec.scope_event(api::Event::Resize {
                surface: native.spec.id,
                width: native.width as f32,
                height: native.height as f32,
                scale: native.scale,
                density,
            });
            if let Some(message) = state.app.event(event) {
                state.message(message, qh);
                return;
            }
        }
        state.draw_all(qh);
    }
}
wayland_client::delegate_noop!(@<A:Application> State<A>:WpFractionalScaleManagerV1);
wayland_client::delegate_noop!(@<A:Application> State<A>:WpViewporter);
wayland_client::delegate_noop!(@<A:Application> State<A>:WpViewport);
