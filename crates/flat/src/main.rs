use direct::Plan;
use fframes::{EncoderOptions, MediaDirectory, RenderOptions, cli};
use fframes_skia_renderer::{
    SkiaFFramesRenderer, SkiaPipelineConcurrencyPolicy, SkiaPipelineConfig, metal::SkiaMetalCtx,
};
use cinema_flat::{FlatVideo, HEIGHT, WIDTH};
use std::process::ExitCode;

fn main() -> ExitCode {
    // Run from the workspace root: media/ holds the fonts, data/ and vocab/ the inputs.
    let plan = Plan::load("data/analysis.json", "vocab").unwrap_or_else(|e| {
        eprintln!("cinema-flat: {e}");
        std::process::exit(1);
    });
    let fonts = MediaDirectory::read_folder("media").expect("media folder");
    let media = fonts.process_media_source().expect("fonts");
    let video = FlatVideo::new(plan);
    let gpu = SkiaMetalCtx::new(WIDTH, HEIGHT).expect("GPU context");

    cli::new(
        &video,
        RenderOptions {
            media: Some(&media),
            default_font: "JetBrains Mono",
            // Near lossless: the lens pass films this file and magnifies it.
            video_encoder_options: EncoderOptions {
                preferred_encoder: Some("libx264"),
                codec_params: Some(&[("crf", "8"), ("preset", "fast"), ("tune", "animation")]),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .backend(
        SkiaFFramesRenderer::new_metal(
            &gpu,
            SkiaPipelineConfig {
                concurrency_policy: SkiaPipelineConcurrencyPolicy::MaxPerformance,
                ..Default::default()
            },
        )
        .expect("skia renderer"),
    )
    .preview(fframes_native_player::cli_preview)
    .default_output("out/pass/flat.mp4")
    .run()
}
