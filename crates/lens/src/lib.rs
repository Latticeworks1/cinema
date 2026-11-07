//! Pass 2: a virtual camera films the flat pass.
//!
//! The shader does the physical part: perspective, depth of field focused on the tracked note,
//! palette, bloom, chromatic aberration, the tear on a lock, and a display model whose subpixel
//! geometry is chosen per panel. Every per-frame value comes from `direct::Plan::frame`.

mod anchors;
mod overlay;

use direct::{DisplayLayout, FrameState, Plan};
use fframes::{
    AudioMap, AudioTimestamp::*, AudioTrack, Color, Duration, FFramesContext, FFramesSyncedVideoFrame,
    Frame, Shader, ShaderUniforms, Svgr, SyncVideoFrameInput, Video,
};

pub const WIDTH: usize = track::output::OUTPUT_WIDTH;
pub const HEIGHT: usize = track::output::OUTPUT_HEIGHT;
pub const MAX_REGIONS: usize = 8;

pub struct LensVideo {
    plan: Plan,
    shader: Shader,
}

impl LensVideo {
    pub fn new(plan: Plan) -> Self {
        Self { plan, shader: Shader::sksl(format!("{}\n{}", track::output::shader_constants(), include_str!("../shaders/lens.sksl"))) }
    }

    /// Shader uniforms of a frame; the flat image is bound by the caller.
    pub fn uniforms(&self, s: &FrameState) -> ShaderUniforms {
        let cam = &s.camera;
        let p = s.stops;
        let l = &s.look;
        let materials = &self.plan.vocab.materials;
        let mat = |i: usize| {
            let m = &materials[i];
            let id = match m.layout {
                DisplayLayout::Stripe => 0.0,
                DisplayLayout::Dot => 1.0,
                DisplayLayout::Delta => 2.0,
                DisplayLayout::Block => 3.0,
            };
            [m.cell, id, m.gap, m.angle]
        };
        let mut u = ShaderUniforms::new()
            .float2("uAim", cam.aim.0, cam.aim.1)
            .float("uZoom", cam.zoom)
            .float3("uRot", cam.tilt_x, cam.tilt_y, cam.roll)
            .float2("uFocus", s.focus.0, s.focus.1)
            .float("uBlur", l.blur)
            .float3("uP0", p[0][0], p[0][1], p[0][2])
            .float3("uP1", p[1][0], p[1][1], p[1][2])
            .float3("uP2", p[2][0], p[2][1], p[2][2])
            .float3("uP3", p[3][0], p[3][1], p[3][2])
            .float3("uP4", p[4][0], p[4][1], p[4][2])
            .float("uLock", f32::from(l.lock))
            .float("uPunch", l.punch)
            .float("uTear", l.tear)
            .float("uCut", l.cut)
            .float("uOut", l.out)
            .float("uFocal", self.plan.dir.camera.focal)
            .float("uBloom", l.bloom)
            .float("uLed", l.led)
            .float("uAberr", l.aberration)
            .float("uGrain", l.grain)
            .float("uVig", l.vignette)
            .float("uSeed", s.index as f32)
            .float("uStage", stage());
        let sm = mat(s.screen_material);
        u = u.float4("uScreenMat", sm[0], sm[1], sm[2], sm[3]);
        for k in 0..MAX_REGIONS {
            let (r, m) = match s.regions.get(k) {
                Some(reg) => (
                    [reg.rect.x, reg.rect.y, reg.rect.right(), reg.rect.bottom()],
                    mat(reg.material),
                ),
                None => ([0.0; 4], sm),
            };
            u = u
                .float4(format!("uRegion{k}"), r[0], r[1], r[2], r[3])
                .float4(format!("uMat{k}"), m[0], m[1], m[2], m[3]);
        }
        u
    }
}

impl Video for LensVideo {
    const FPS: usize = track::FPS;
    const WIDTH: usize = WIDTH;
    const HEIGHT: usize = HEIGHT;
    const BACKGROUND_COLOR: Color = Color::BLACK;

    fn duration(&self) -> Duration<'_> {
        Duration::Frames(self.plan.frames())
    }

    fn audio(&self) -> AudioMap<'_> {
        if self.plan.track().has_audio() {
            AudioMap::from([AudioTrack::new("music.wav", Second(0.)..Eof)])
        } else {
            AudioMap::none()
        }
    }

    fn render_frame<'a>(&'a self, frame: Frame, ctx: &FFramesContext<'a, '_>) -> Svgr<'a> {
        let s = self.plan.frame(frame.index);
        let offset: f32 = std::env::var("CINEMA_OFFSET_S").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let input = SyncVideoFrameInput { start_from: offset, looping: false, editor_fallback_image: None };
        // A root without <svg> makes a full render hang instead of failing (fframes 1.2.0),
        // so a missing frame draws black.
        let Some(flat) = frame.get_synced_video_frame(ctx, "flat.mp4", &input) else {
            return fframes::svgr!(
                <svg xmlns="http://www.w3.org/2000/svg" width={WIDTH} height={HEIGHT}>
                    <rect width={WIDTH} height={HEIGHT} fill="#000000" />
                </svg>
            );
        };
        let flat = flat.into_image();
        let layer = self.shader.draw(&frame, self.uniforms(&s).image("iChannel0", &flat));
        fframes::svgr!(
            <svg xmlns="http://www.w3.org/2000/svg" width={WIDTH} height={HEIGHT}>
                <image href={layer.href()} x="0" y="0" width={WIDTH} height={HEIGHT} />
                {overlay::hud(&self.plan, &s)}
                {if stage() >= 4.0 { overlay::tracker(&self.plan, &s) } else { Svgr::empty() }}
            </svg>
        )
    }
}

/// `LENS_STAGE=0..3` renders the shader up to one step (see lens.sksl); unset = all.
fn stage() -> f32 {
    std::env::var("LENS_STAGE").ok().and_then(|v| v.parse().ok()).unwrap_or(4.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video() -> LensVideo {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        LensVideo::new(Plan::load(format!("{root}/data/analysis.json"), format!("{root}/vocab")).unwrap())
    }

    #[test]
    fn the_shader_compiles() {
        let v = video();
        fframes_skia_renderer::render::compile_shader(&v.shader).expect("lens.sksl compiles with Skia");
    }

    #[test]
    fn every_frame_builds_uniforms_without_panicking() {
        let v = video();
        for f in (0..v.plan.frames()).step_by(7) {
            let _ = v.uniforms(&v.plan.frame(f));
        }
    }
}
