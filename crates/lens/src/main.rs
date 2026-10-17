use direct::Plan;
use packet::Packet;
use fframes::{CombinedMediaProvider, EncoderOptions, MediaDirectory, MediaProvider, RenderOptions, cli};
use fframes_skia_renderer::{
    SkiaFFramesRenderer, SkiaPipelineConcurrencyPolicy, SkiaPipelineConfig, metal::SkiaMetalCtx,
};
use cinema_lens::{HEIGHT, LensVideo, WIDTH};
use std::process::ExitCode;

fn main() -> ExitCode {
    // The packet (CINEMA_PACKET, default packets/default.json) names every input; its pass
    // directory holds flat.mp4 and music.wav (tools/render.sh).
    let (packet, plan) = Packet::from_env().and_then(|p| Plan::from_packet(&p).map(|plan| (p, plan))).unwrap_or_else(|e| {
        eprintln!("cinema-lens: {e}");
        std::process::exit(1);
    });
    let font = plan.dir.font.clone();
    let video_out = packet.video_path().to_string_lossy().into_owned();
    let dir = MediaDirectory::read_folder(packet.pass_dir()).expect("pass directory: run tools/render.sh first");
    let pass = dir.process_media_source().expect("out/pass media");
    let fonts_dir = MediaDirectory::read_folder(packet.media()).expect("media folder");
    let fonts = fonts_dir.process_media_source().expect("fonts");
    let media = CombinedMediaProvider::from([&pass as &dyn MediaProvider, &fonts]);
    let video = LensVideo::new(plan);
    let gpu = SkiaMetalCtx::new(WIDTH, HEIGHT).expect("GPU context");

    cli::new(
        &video,
        RenderOptions {
            media: Some(&media),
            default_font: &font,
            video_encoder_options: EncoderOptions {
                preferred_encoder: Some("libx264"),
                codec_params: Some(&[("crf", "16"), ("preset", "slow"), ("tune", "grain")]),
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
    .default_output(&video_out)
    .run()
}
