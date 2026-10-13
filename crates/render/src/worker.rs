//! Drawing on a thread of its own (B-41). Loading the fonts takes up to
//! seconds, scaling a cover or a first use of a fallback font milliseconds,
//! and none of it may hold up OBS's graphics thread, which only asks for
//! frames and collects them.

use std::io;
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use mpris_core::content::Content;
use mpris_core::model::Settings;

use crate::art::ArtCache;
use crate::draw::draw;
use crate::frame::Frame;
use crate::layout::Layout;
use crate::text::Fonts;

/// The drawing thread, shared by every source. Dropping it stops the
/// thread, once the frame in progress is drawn.
pub struct Worker {
    messages: Sender<Message>,
    thread: Option<JoinHandle<()>>,
}

/// One source's side of the worker: asks for frames and collects them.
pub struct Painter {
    shared: Arc<Shared>,
    messages: Sender<Message>,
}

enum Message {
    Draw(Arc<Shared>),
    Stop,
}

/// What a painter and the worker exchange.
#[derive(Default)]
struct Shared {
    /// The latest request not yet drawn. A newer one replaces it, so the
    /// worker never draws a frame already out of date.
    request: Mutex<Option<Request>>,
    /// The latest frame drawn and not yet collected.
    done: Mutex<Option<Frame>>,
    /// The cover of this source, scaled.
    art: Mutex<ArtCache>,
}

struct Request {
    layout: Layout,
    content: Content,
    settings: Settings,
}

impl Worker {
    /// Starts the thread, which first gets its fonts from `fonts`.
    pub fn start(fonts: impl FnOnce() -> Fonts + Send + 'static) -> io::Result<Worker> {
        let (messages, inbox) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("mpris-render".into())
            .spawn(move || run(fonts(), &inbox))?;
        Ok(Worker {
            messages,
            thread: Some(thread),
        })
    }

    pub fn painter(&self) -> Painter {
        Painter {
            shared: Arc::default(),
            messages: self.messages.clone(),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Painters may outlive the worker, so their senders cannot be what
        // ends the thread.
        let _ = self.messages.send(Message::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Painter {
    /// Asks for `content` drawn, in place of any request not drawn yet.
    pub fn request(&self, layout: Layout, content: Content, settings: Settings) {
        *lock(&self.shared.request) = Some(Request {
            layout,
            content,
            settings,
        });
        // A stopped worker draws nothing more, which is all a source can
        // expect at that point.
        let _ = self.messages.send(Message::Draw(Arc::clone(&self.shared)));
    }

    /// The latest frame drawn since the previous call, if any.
    pub fn take(&self) -> Option<Frame> {
        lock(&self.shared.done).take()
    }
}

fn run(mut fonts: Fonts, inbox: &Receiver<Message>) {
    let mut timings = 0;
    while let Ok(Message::Draw(shared)) = inbox.recv() {
        // Requests replaced before the worker got to them leave nothing.
        let Some(request) = lock(&shared.request).take() else {
            continue;
        };
        let started = Instant::now();
        let drawn = panic::catch_unwind(AssertUnwindSafe(|| {
            let art = &mut lock(&shared.art);
            draw(
                &request.layout,
                &request.content,
                &request.settings,
                &mut fonts,
                art,
            )
        }));
        match drawn {
            Ok(frame) => {
                // The first frames with a cover tell what drawing costs, the
                // cover scaled then reused.
                if timings < 2 && has_art(&request.content) {
                    timings += 1;
                    let (w, h) = (frame.width(), frame.height());
                    log::debug!(
                        "{w}x{h} frame with a cover drawn in {:?}",
                        started.elapsed()
                    );
                }
                *lock(&shared.done) = Some(frame);
            }
            // One bad frame must not stop every source from updating (B-42).
            Err(_) => log::error!("drawing a frame failed"),
        }
    }
}

fn has_art(content: &Content) -> bool {
    matches!(content, Content::Track(track) if track.art.is_some())
}

/// Locks `mutex` even after a panic while held: every value it guards is
/// replaced whole, never left half written.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::layout::layout;

    fn job(width: u32) -> (Layout, Content, Settings) {
        let settings = Settings {
            width,
            ..Settings::default()
        };
        (layout(&settings), Content::Message(String::new()), settings)
    }

    fn ask(painter: &Painter, width: u32) {
        let (layout, content, settings) = job(width);
        painter.request(layout, content, settings);
    }

    /// The next frame of `painter`, waiting for the worker to draw it.
    fn wait(painter: &Painter) -> Frame {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(frame) = painter.take() {
                return frame;
            }
            assert!(Instant::now() < deadline, "no frame drawn");
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// A worker that loads its fonts only once the returned sender says so.
    fn gated() -> (Worker, Sender<()>) {
        let (open, gate): (Sender<()>, Receiver<()>) = mpsc::channel();
        let worker = Worker::start(move || {
            let _ = gate.recv();
            Fonts::none()
        });
        (worker.expect("worker"), open)
    }

    #[test]
    fn a_request_comes_back_drawn() {
        let worker = Worker::start(Fonts::none).expect("worker");
        let painter = worker.painter();
        ask(&painter, 300);
        let (layout, content, settings) = job(300);
        let expected = draw(
            &layout,
            &content,
            &settings,
            &mut Fonts::none(),
            &mut ArtCache::new(),
        );
        assert_eq!(wait(&painter), expected);
        assert!(painter.take().is_none(), "a frame is handed out once");
    }

    #[test]
    fn only_the_latest_request_is_drawn() {
        let (worker, open) = gated();
        let painter = worker.painter();
        ask(&painter, 300);
        ask(&painter, 400);
        open.send(()).expect("open");
        assert_eq!(wait(&painter).width(), 400);
        thread::sleep(Duration::from_millis(50));
        assert!(painter.take().is_none(), "the older request is dropped");
    }

    #[test]
    fn each_painter_gets_its_own_frames() {
        let worker = Worker::start(Fonts::none).expect("worker");
        let (first, second) = (worker.painter(), worker.painter());
        ask(&first, 300);
        ask(&second, 400);
        assert_eq!(wait(&first).width(), 300);
        assert_eq!(wait(&second).width(), 400);
    }

    #[test]
    fn requests_wait_for_the_fonts() {
        let (worker, open) = gated();
        let painter = worker.painter();
        ask(&painter, 300);
        thread::sleep(Duration::from_millis(50));
        assert!(painter.take().is_none());
        open.send(()).expect("open");
        assert_eq!(wait(&painter).width(), 300);
    }

    #[test]
    fn fonts_load_on_the_worker_thread() {
        let (sender, receiver) = mpsc::channel();
        let _worker = Worker::start(move || {
            let _ = sender.send(thread::current().id());
            Fonts::none()
        })
        .expect("worker");
        let loader = receiver.recv().expect("fonts loaded");
        assert_ne!(loader, thread::current().id());
    }

    #[test]
    fn stopping_does_not_wait_for_the_painters() {
        let worker = Worker::start(Fonts::none).expect("worker");
        let painter = worker.painter();
        drop(worker);
        ask(&painter, 300);
        thread::sleep(Duration::from_millis(20));
        assert!(painter.take().is_none());
    }

    #[test]
    fn a_painter_gone_before_its_frame_is_drawn() {
        let (worker, open) = gated();
        let painter = worker.painter();
        ask(&painter, 300);
        drop(painter);
        open.send(()).expect("open");
        drop(worker);
    }

    #[test]
    fn stopping_while_the_fonts_load() {
        let (worker, open) = gated();
        let stopper = thread::spawn(move || drop(worker));
        thread::sleep(Duration::from_millis(20));
        open.send(()).expect("open");
        stopper.join().expect("stopped");
    }
}
