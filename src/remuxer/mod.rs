use anyhow::{anyhow, Context};
use async_process::Command;
use bytes::Bytes;
use futures::future::try_join;
use futures::{AsyncReadExt, FutureExt, Stream, TryFutureExt};
use std::collections::HashMap;
use std::process::{ExitStatus, Stdio};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::{pin, try_join};
use tokio_stream::StreamExt;
use tokio_util::codec::{BytesCodec, FramedRead};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tracing::{info, trace};

async fn write_stream(
    stream: impl Stream<Item = anyhow::Result<Bytes>>,
    dest: impl AsyncWrite,
) -> anyhow::Result<()> {
    pin!(stream);
    pin!(dest);
    while let Some(value) = stream.next().await {
        let value = value.context("Reading from stream")?;
        trace!("Write {} bytes!!", value.len());
        dest.write_all(value.as_ref())
            .await
            .context("Writing to stream")?;
    }
    Ok(())
}

async fn pump_ffmpeg_stdout(reader: impl AsyncBufRead) -> anyhow::Result<()> {
    pin!(reader);
    let mut lines = reader.lines();

    let mut progress: HashMap<String, String> = HashMap::new();

    while let Some(line) = lines.next_line().await? {
        let (k, v) = line
            .split_once('=')
            .ok_or_else(|| anyhow!("ffmpeg stdout was not k=v-formatted"))?;

        if k == "progress" {
            let fps = progress.get("fps").map(|v| v.as_str()).unwrap_or("");
            let speed = progress.get("speed").map(|v| v.as_str()).unwrap_or("");
            let out_time = progress.get("out_time").map(|v| v.as_str()).unwrap_or("");
            info!(
                "ffmpeg(progress) fps={:5} speed={:5} out_time={}",
                fps, speed, out_time
            )
        } else {
            progress.insert(k.to_string(), v.to_string());
        }
    }
    Ok(())
}

async fn pump_ffmpeg_stderr(reader: impl AsyncBufRead) -> anyhow::Result<()> {
    pin!(reader);
    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await? {
        info!("ffmpeg(err): {}", line)
    }
    Ok(())
}

pub async fn remux(
    video: impl Stream<Item = anyhow::Result<Bytes>>,
    audio: impl Stream<Item = anyhow::Result<Bytes>>,
    dest: impl AsyncWrite,
) -> anyhow::Result<()> {
    // TODO: get this from config or smth
    let ffmpeg = which::which("ffmpeg").context("Locating the ffmpeg binary")?;

    info!("Found ffmpeg: {:?}", &ffmpeg);

    let tmp = tempdir::TempDir::new("yasd_remux")?;

    info!("Created temp dir ffmpeg: {:?}", &tmp);

    let video_in_pipe_name = tmp.path().join("video_in");
    let audio_in_pipe_name = tmp.path().join("audio_in");
    let muxed_out_pipe_name = tmp.path().join("muxed_out");

    unix_named_pipe::create(&video_in_pipe_name, Some(0o600)).context("Creating video_in pipe")?;
    unix_named_pipe::create(&audio_in_pipe_name, Some(0o600)).context("Creating audio_in pipe")?;
    unix_named_pipe::create(&muxed_out_pipe_name, Some(0o600))
        .context("Creating muxed_out pipe")?;

    info!("Created pipes, spawning ffmpeg...");

    let mut ffmpeg = Command::new(ffmpeg)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // yes, "overwrite" it
        .arg("-y")
        // be nice to the machine
        .arg("-progress")
        .arg("-")
        // read from pipes
        .arg("-i")
        .arg(&video_in_pipe_name)
        .arg("-i")
        .arg(&audio_in_pipe_name)
        // don't reencode
        .arg("-c")
        .arg("copy")
        // mux as streamable mp4
        .arg("-f")
        .arg("ismv")
        // write to pipe
        .arg(&muxed_out_pipe_name)
        .spawn()
        .context("Spawning ffmpeg")?;

    pin!(dest);

    info!("Piping...");

    let ffmpeg_status_fut = ffmpeg.status();
    let stdout = tokio::io::BufReader::new(ffmpeg.stdout.unwrap().compat());
    let stderr = tokio::io::BufReader::new(ffmpeg.stderr.unwrap().compat());

    try_join! {
        async {
            pump_ffmpeg_stdout(stdout).await.context("Pumping ffmpeg stdout")
        },
        async {
            pump_ffmpeg_stderr(stderr).await.context("Pumping ffmpeg stderr")
        },
        async {
            ffmpeg_status_fut.await.context("Waiting for ffmpeg status")
                .and_then(|s: ExitStatus| -> anyhow::Result<()> {
                    if s.success() {
                        Ok(())
                    } else {
                        Err(anyhow!("ffmpeg exited with bad ExitStatus: {}", s))
                    }
                })
        },
        async {
            // opening pipes in async context is important!
            // that's because opening them blocks until the other side opens them and ffmpeg does not open those at the beginning
            OpenOptions::new()
                .write(true)
                .open(video_in_pipe_name)
                .map_err(anyhow::Error::new)
                .and_then(|video_in_pipe| async move {
                    write_stream(video, video_in_pipe)
                        .await
                        .context("Piping video stream to ffmpeg")
                })
                .await
                .context("Piping video stream to ffmpeg")
        },
        async {
            OpenOptions::new()
                .write(true)
                .open(audio_in_pipe_name)
                .map_err(anyhow::Error::new)
                .and_then(|audio_in_pipe| async move {
                    write_stream(audio, audio_in_pipe).await
                })
                .await
                .context("Piping audio stream to ffmpeg")
        },
        async {
            OpenOptions::new()
                .read(true)
                .open(muxed_out_pipe_name)
                .map_err(anyhow::Error::new)
                .and_then(|mut muxed_out_pipe| async move {
                    tokio::io::copy(&mut muxed_out_pipe, &mut dest).await
                        .context("Piping muxed stream from ffmpeg to output")
                })
                .await
                .context("Piping muxed stream from ffmpeg to output")
        }
    }?;

    Ok(())
}
