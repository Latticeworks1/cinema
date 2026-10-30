use direct::Plan;
use packet::Packet;
use fframes::{CombinedMediaProvider, EncoderOptions, MediaDirectory, MediaProvider, RenderOptions, cli};
use fframes_skia_renderer::{
    SkiaFFramesRenderer, SkiaPipelineConcurrencyPolicy, SkiaPipelineConfig, metal::SkiaMetalCtx,
};
use cinema_flat::{FlatVideo, HEIGHT, WIDTH};
use std::process::ExitCode;

fn main() -> ExitCode {
    // The packet (CINEMA_PACKET, default packets/default.json) names every input.
    let (packet, plan) = Packet::from_env().and_then(|p| Plan::from_packet(&p).map(|plan| (p, plan))).unwrap_or_else(|e| {
        eprintln!("cinema-flat: {e}");
        std::process::exit(1);
    });
    let font = plan.dir.font.clone();
    let flat_out = packet.flat_path().to_string_lossy().into_owned();
    // The pass directory holds music.wav (written by `packet prepare`) for the spectrum; the
    // fonts come from the media folder, as in the lens pass.
    let pass_dir = MediaDirectory::read_folder(packet.pass_dir()).expect("pass directory: run `packet prepare` first");
    let pass = pass_dir.process_media_source().expect("pass media");
    let fonts_dir = MediaDirectory::read_folder(packet.media()).expect("media folder");
    let fonts = fonts_dir.process_media_source().expect("fonts");
    let media = CombinedMediaProvider::from([&pass as &dyn MediaProvider, &fonts]);
    let video = FlatVideo::new(plan);
    let gpu = SkiaMetalCtx::new(WIDTH, HEIGHT).expect("GPU context");

    cli::new(
        &video,
        RenderOptions {
            media: Some(&media),
            default_font: &font,
            // Near lossless: the lens pass films this file and magnifies it.
            video_encoder_options: EncoderOptions {
                preferred_encoder: Some(track::output::FLAT_CODEC),
                codec_params: Some(track::output::FLAT_PARAMS),
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
    .default_output(&flat_out)
    .run()
}
