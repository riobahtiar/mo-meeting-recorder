//! Local sockets for the app's live state: one JSON line per tick, plus
//! one-line commands back. Unix domain sockets on Unix; named pipes arrive
//! with the Windows shell (plan 16), until when every constructor here
//! reports unsupported and the app runs without a menu bar item.
//!
//! The surface mirrors exactly what `ipc` needs — bind, connect, accept,
//! clone, write timeout — so the protocol module never sees a `cfg`.

use std::io;
use std::path::Path;

#[cfg(unix)]
mod imp {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::time::Duration;

    pub struct Listener(UnixListener);
    pub struct Stream(UnixStream);

    pub fn bind(path: impl AsRef<Path>) -> io::Result<Listener> {
        UnixListener::bind(path).map(Listener)
    }

    pub fn connect(path: impl AsRef<Path>) -> io::Result<Stream> {
        UnixStream::connect(path).map(Stream)
    }

    impl Listener {
        pub fn accept(&self) -> io::Result<Stream> {
            self.0.accept().map(|(stream, _)| Stream(stream))
        }
    }

    impl Stream {
        pub fn try_clone(&self) -> io::Result<Stream> {
            self.0.try_clone().map(Stream)
        }

        pub fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
            self.0.set_write_timeout(timeout)
        }
    }

    impl Read for Stream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.0.read(buf)
        }
    }

    impl Write for Stream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.0.flush()
        }
    }
}

#[cfg(not(unix))]
mod imp {
    use super::*;
    use std::io::{Read, Write};
    use std::time::Duration;

    fn unsupported(what: &str) -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            format!("local sockets need the Windows shell: {what} (plan 16)"),
        )
    }

    pub struct Listener;
    pub struct Stream;

    pub fn bind(path: impl AsRef<Path>) -> io::Result<Listener> {
        let _ = path;
        Err(unsupported("bind"))
    }

    pub fn connect(path: impl AsRef<Path>) -> io::Result<Stream> {
        let _ = path;
        Err(unsupported("connect"))
    }

    impl Listener {
        pub fn accept(&self) -> io::Result<Stream> {
            Err(unsupported("accept"))
        }
    }

    impl Stream {
        pub fn try_clone(&self) -> io::Result<Stream> {
            Err(unsupported("clone"))
        }

        pub fn set_write_timeout(&self, _timeout: Option<Duration>) -> io::Result<()> {
            Err(unsupported("timeout"))
        }
    }

    impl Read for Stream {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(unsupported("read"))
        }
    }

    impl Write for Stream {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(unsupported("write"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(unsupported("flush"))
        }
    }
}

pub use imp::{Listener, Stream, bind, connect};
