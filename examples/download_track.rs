//! A simple example that downloads a track, album or playlist from Spotify.
//!
//! You can run this example from the root of the repository with the following command:
//!
//! ```sh
//! cargo run --example download_track -- --uri <URI>
//! ```
//!
//! e.g.
//!
//! ```sh
//! cargo run --example download_track -- --uri spotify:track:4uLU6hMCjMI75M1A2tKUQC
//! ```
//!
//! You can also run it with a playlist URI:
//!
//! ```sh
//! cargo run --example download_track -- --uri spotify:playlist:37i9dQZEVXbMDoHDwVN2tF
//! ```
//!
//! Authentication is handled by caching credentials in the `.cache` directory. If you are running
//! this example for the first time, you will be prompted to login in your browser.

use std::{fs::File, io, path::Path};

use clap::Parser;
use librespot::{
    audio::{AudioDecrypt, AudioFile},
    core::{
        Error, SpotifyUri, authentication::Credentials, cache::Cache, config::SessionConfig,
        session::Session,
    },
    metadata::{Album, Metadata, Playlist, Track, audio::file::AudioFileFormat},
};
use log::LevelFilter;
use rand::RngCore;
use std::process::Command;

const CACHE: &str = ".cache";
const CACHE_FILES: &str = ".cache/files";

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// The URI of the track, album or playlist to download.
    #[clap(short, long)]
    uri: String,
    #[clap(short, long)]
    dest_dir: String,
}

fn stream_data_rate(format: AudioFileFormat) -> Option<usize> {
    let kbps = match format {
        AudioFileFormat::OGG_VORBIS_96 => 12.,
        AudioFileFormat::OGG_VORBIS_160 => 20.,
        AudioFileFormat::OGG_VORBIS_320 => 40.,
        AudioFileFormat::MP3_256 => 32.,
        AudioFileFormat::MP3_320 => 40.,
        AudioFileFormat::MP3_160 => 20.,
        AudioFileFormat::MP3_96 => 12.,
        AudioFileFormat::MP3_160_ENC => 20.,
        AudioFileFormat::AAC_24 => 3.,
        AudioFileFormat::AAC_48 => 6.,
        AudioFileFormat::AAC_160 => 20.,
        AudioFileFormat::AAC_320 => 40.,
        AudioFileFormat::MP4_128 => 16.,
        AudioFileFormat::OTHER5 => 40.,
        AudioFileFormat::FLAC_FLAC => 112., // assume 900 kbit/s on average
        AudioFileFormat::XHE_AAC_12 => 1.5,
        AudioFileFormat::XHE_AAC_16 => 2.,
        AudioFileFormat::XHE_AAC_24 => 3.,
        AudioFileFormat::FLAC_FLAC_24BIT => 3.,
    };
    let data_rate: f32 = kbps * 1024.;
    Some(data_rate.ceil() as usize)
}

async fn retry<T, E, F, Fut>(mut operation: F) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Debug,
{
    let max_attempts = 5;
    let mut rng = rand::thread_rng();
    for attempt in 1..=max_attempts {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(err) => {
                println!("Attempt {} failed: {:?}", attempt, err);
                if attempt == max_attempts {
                    return Err(err);
                }
            }
        }
        let jitter = rng.next_u64() % 5000;
        let dur = std::time::Duration::from_millis(5000 * (1 << (attempt - 1)) + jitter);
        std::thread::sleep(dur);
    }
    unreachable!()
}

fn format_extension(format: AudioFileFormat) -> Option<String> {
    match format {
        AudioFileFormat::OGG_VORBIS_96 => Some("ogg".to_string()),
        AudioFileFormat::OGG_VORBIS_160 => Some("ogg".to_string()),
        AudioFileFormat::OGG_VORBIS_320 => Some("ogg".to_string()),
        AudioFileFormat::MP3_256 => Some("mp3".to_string()),
        AudioFileFormat::MP3_320 => Some("mp3".to_string()),
        AudioFileFormat::MP3_160 => Some("mp3".to_string()),
        AudioFileFormat::MP3_96 => Some("mp3".to_string()),
        AudioFileFormat::MP3_160_ENC => Some("mp3".to_string()),
        AudioFileFormat::AAC_24 => Some("aac".to_string()),
        AudioFileFormat::AAC_48 => Some("aac".to_string()),
        AudioFileFormat::AAC_160 => Some("aac".to_string()),
        AudioFileFormat::AAC_320 => Some("aac".to_string()),
        AudioFileFormat::MP4_128 => Some("mp4".to_string()),
        AudioFileFormat::FLAC_FLAC => Some("flac".to_string()),
        _ => None,
    }
}

async fn download_track(base_dir: &str, track: Track, session: &Session) -> Result<(), Error> {
    let artists = track
        .artists
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let spotify_id = if let SpotifyUri::Track { id } = track.id {
        id
    } else {
        return Err(Error::invalid_argument("Not a track URI"));
    };

    let formats = [
        AudioFileFormat::FLAC_FLAC,
        AudioFileFormat::OGG_VORBIS_320,
        AudioFileFormat::MP3_320,
        AudioFileFormat::MP3_256,
        AudioFileFormat::OGG_VORBIS_160,
        AudioFileFormat::MP3_160,
        AudioFileFormat::OGG_VORBIS_96,
        AudioFileFormat::MP3_96,
    ];

    let (format, file_id) = formats
        .iter()
        .find_map(|format| match track.files.get(format) {
            Some(&file_id) => Some((*format, file_id)),
            _ => None,
        })
        .ok_or(Error::unavailable(format!(
            "None of the formats matched for {}: {:?}",
            &track.name, &track.files
        )))?;

    let ext = format_extension(format).ok_or(Error::unavailable(format!(
        "Could not find extension for {:?}",
        format
    )))?;
    let filename = format!(
        "{}/{}_{}_{}.{}",
        base_dir,
        artists,
        track.name,
        spotify_id.to_base62()?,
        ext
    );

    if Path::new(&filename).exists() {
        println!("Track {} already exists, skipping", filename);
        return Ok(());
    }

    println!("Downloading track {}...", filename);

    let bytes_per_second = stream_data_rate(format).ok_or(Error::unavailable(format!(
        "Could not convert provided format: {:?}",
        format
    )))?;
    let enctypted_file = retry(|| AudioFile::open(session, file_id, bytes_per_second)).await?;

    let key = retry(|| session.audio_key().request(spotify_id, file_id)).await?;
    let mut decrypted_file = AudioDecrypt::new(Some(key), enctypted_file);

    let mut file = File::create(&filename)?;
    io::copy(&mut decrypted_file, &mut file)?;

    println!("Track {} downloaded successfully", filename);

    tag_file(format, &filename, &artists, &track).await?;

    Ok(())
}

async fn tag_file(
    format: AudioFileFormat,
    filename: &str,
    artist: &str,
    track: &Track,
) -> Result<(), Error> {
    match format {
        AudioFileFormat::OGG_VORBIS_320
        | AudioFileFormat::OGG_VORBIS_160
        | AudioFileFormat::OGG_VORBIS_96 => (),
        _ => {
            return Err(Error::invalid_argument(format!(
                "Cannot process format {:?}",
                format
            )));
        }
    }
    let output = Command::new("vorbiscomment")
        .args([
            "-a",
            filename,
            "-t",
            format!("ARTIST={}", artist).as_str(),
            "-t",
            format!("TRACKNUMBER={}", track.number).as_str(),
            "-t",
            format!("ALBUM={}", &track.album.name).as_str(),
            "-t",
            format!("TITLE={}", &track.name).as_str(),
        ])
        .output()?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(Error::internal(format!("vorbiscomment failed: {}", stderr)))
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::builder()
        .filter_module("librespot", LevelFilter::Info)
        .init();

    let args = Args::parse();
    let spotify_uri = SpotifyUri::from_uri(&args.uri)?;
    let base_dir = args.dest_dir;

    let session_config = SessionConfig::default();
    // TODO: investigate why the credentials are not actually stored
    let cache = Cache::new(Some(CACHE), Some(CACHE), Some(CACHE_FILES), None)?;
    let credentials = cache
        .credentials()
        .ok_or(Error::unavailable("credentials not cached"))
        .or_else(|_| {
            librespot_oauth::OAuthClientBuilder::new(
                &session_config.client_id,
                "http://127.0.0.1:8898/login",
                vec!["streaming"],
            )
            // .open_in_browser()
            .build()?
            .get_access_token()
            .map(|t| Credentials::with_access_token(t.access_token))
        })?;

    let session = Session::new(session_config, Some(cache));
    session.connect(credentials, false).await?;

    match spotify_uri {
        SpotifyUri::Track { id } => {
            let uri = SpotifyUri::Track { id };
            let track = Track::get(&session, &uri).await?;
            download_track(&base_dir, track, &session).await?;
        }
        SpotifyUri::Album { id } => {
            let uri = SpotifyUri::Album { id };
            let album = Album::get(&session, &uri).await?;
            println!("Downloading album {}...", album.name);
            for track_uri in album.tracks() {
                let track = Track::get(&session, track_uri).await?;
                download_track(&base_dir, track, &session).await?;
            }
        }
        SpotifyUri::Playlist { id, .. } => {
            let uri = SpotifyUri::Playlist { id, user: None };
            let playlist = Playlist::get(&session, &uri).await?;
            println!("Downloading playlist {}...", playlist.name());
            for track_uri in playlist.tracks() {
                let track = Track::get(&session, track_uri).await?;
                retry(|| download_track(&base_dir, track.clone(), &session)).await?;
            }
        }
        _ => {
            println!("Unsupported URI type: {:?}", spotify_uri);
        }
    }

    Ok(())
}
