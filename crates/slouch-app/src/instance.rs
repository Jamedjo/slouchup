//! One SlouchUp at a time. Starting it again while it runs opens the running one's window, on
//! the view asked for, which is the way back in on panels that never pass on a click.
//!
//! The running one listens on a loopback port, written to the cache directory. All it takes is a
//! view's name, so anything else on the computer that connects can do no more than open it.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

use futures_channel::mpsc::UnboundedSender;

use crate::ui::Page;

const PORT_FILE: &str = "port";
const PATIENCE: Duration = Duration::from_secs(2);

/// Held while this is the running SlouchUp.
pub struct Instance {
    _lock: File,
}

/// Become the running SlouchUp, passing views asked for by later starts to `pages`, or `None`
/// if one is already running.
pub fn claim(cache: &Path, pages: UnboundedSender<Page>) -> Option<Instance> {
    let lock = File::create(cache.join("lock")).ok()?;
    lock.try_lock().ok()?;
    if let Err(error) = listen(cache, pages) {
        tracing::warn!("couldn't listen for SlouchUp being started again: {error}");
    }
    Some(Instance { _lock: lock })
}

fn listen(cache: &Path, pages: UnboundedSender<Page>) -> std::io::Result<()> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    std::fs::write(
        cache.join(PORT_FILE),
        listener.local_addr()?.port().to_string(),
    )?;
    std::thread::Builder::new()
        .name("slouch-relaunch".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                if let Some(page) = read_page(stream) {
                    let _ = pages.unbounded_send(page);
                }
            }
        })?;
    Ok(())
}

fn read_page(stream: TcpStream) -> Option<Page> {
    stream.set_read_timeout(Some(PATIENCE)).ok()?;
    let mut line = String::new();
    BufReader::new(stream.take(64)).read_line(&mut line).ok()?;
    parse(line.trim())
}

/// Ask the running SlouchUp to open its window on `page`, and say whether it was asked.
pub fn show_running(cache: &Path, page: Page) -> bool {
    let Some(port) = std::fs::read_to_string(cache.join(PORT_FILE))
        .ok()
        .and_then(|text| text.trim().parse::<u16>().ok())
    else {
        return false;
    };
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&address, PATIENCE)
        .and_then(|mut stream| writeln!(stream, "{}", name(page)))
        .is_ok()
}

fn name(page: Page) -> &'static str {
    match page {
        Page::Camera => "camera",
        Page::History => "history",
        Page::Settings => "settings",
    }
}

fn parse(name: &str) -> Option<Page> {
    Page::ALL.into_iter().find(|page| self::name(*page) == name)
}

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;

    use super::*;

    #[test]
    fn names_round_trip() {
        for page in Page::ALL {
            assert_eq!(parse(name(page)), Some(page));
        }
        assert_eq!(parse("anything else"), None);
    }

    #[test]
    fn a_second_start_reaches_the_first() {
        let cache = std::env::temp_dir().join(format!("slouchup-instance-{}", std::process::id()));
        std::fs::create_dir_all(&cache).unwrap();
        let (sender, mut pages) = futures_channel::mpsc::unbounded();
        let first = claim(&cache, sender).expect("nothing else holds the lock");
        let (unused, _) = futures_channel::mpsc::unbounded();
        assert!(claim(&cache, unused).is_none());
        assert!(show_running(&cache, Page::Settings));
        let asked = block_on(pages.next());
        assert_eq!(asked, Some(Page::Settings));
        drop(first);
        let _ = std::fs::remove_dir_all(&cache);
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        let mut future = std::pin::pin!(future);
        let waker = std::task::Waker::noop();
        let mut context = std::task::Context::from_waker(waker);
        loop {
            if let std::task::Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return output;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
