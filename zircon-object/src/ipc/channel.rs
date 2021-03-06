use {
    crate::object::*,
    alloc::collections::VecDeque,
    alloc::sync::{Arc, Weak},
    alloc::vec::Vec,
    spin::Mutex,
};

pub type Channel = Channel_<MessagePacket>;

/// Bidirectional interprocess communication
pub struct Channel_<T> {
    base: KObjectBase,
    peer: Weak<Channel_<T>>,
    recv_queue: Arc<Mutex<VecDeque<T>>>,
}

impl_kobject!(Channel);

impl<T> Channel_<T> {
    /// Create a channel and return a pair of its endpoints
    #[allow(unsafe_code)]
    pub fn create() -> (Arc<Self>, Arc<Self>) {
        let mut channel0 = Arc::new(Channel_ {
            base: KObjectBase::with_signal(Signal::WRITABLE),
            peer: Weak::default(),
            recv_queue: Default::default(),
        });
        let channel1 = Arc::new(Channel_ {
            base: KObjectBase::with_signal(Signal::WRITABLE),
            peer: Arc::downgrade(&channel0),
            recv_queue: Default::default(),
        });
        // no other reference of `channel0`
        unsafe {
            Arc::get_mut_unchecked(&mut channel0).peer = Arc::downgrade(&channel1);
        }
        (channel0, channel1)
    }

    /// Read a packet from the channel
    pub fn read(&self) -> ZxResult<T> {
        let mut recv_queue = self.recv_queue.lock();
        if let Some(msg) = recv_queue.pop_front() {
            if recv_queue.is_empty() {
                self.base.signal_clear(Signal::READABLE);
            }
            return Ok(msg);
        }
        if self.peer_closed() {
            Err(ZxError::PEER_CLOSED)
        } else {
            Err(ZxError::SHOULD_WAIT)
        }
    }

    /// Write a packet to the channel
    pub fn write(&self, msg: T) -> ZxResult<()> {
        let peer = self.peer.upgrade().ok_or(ZxError::PEER_CLOSED)?;
        let mut send_queue = peer.recv_queue.lock();
        send_queue.push_back(msg);
        if send_queue.len() == 1 {
            peer.base.signal_set(Signal::READABLE);
        }
        Ok(())
    }

    /// Is peer channel closed?
    fn peer_closed(&self) -> bool {
        self.peer.strong_count() == 0
    }
}

impl<T> Drop for Channel_<T> {
    fn drop(&mut self) {
        if let Some(peer) = self.peer.upgrade() {
            peer.base.signal_set(Signal::PEER_CLOSED);
        }
    }
}

#[derive(Default)]
pub struct MessagePacket {
    pub data: Vec<u8>,
    pub handles: Vec<Handle>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use core::sync::atomic::*;

    #[test]
    fn read_write() {
        let (channel0, channel1) = Channel::create();
        // write a message to each other
        channel0
            .write(MessagePacket {
                data: Vec::from("hello 1"),
                handles: Vec::new(),
            })
            .unwrap();
        channel1
            .write(MessagePacket {
                data: Vec::from("hello 0"),
                handles: Vec::new(),
            })
            .unwrap();

        // read message should success
        let recv_msg = channel1.read().unwrap();
        assert_eq!(recv_msg.data.as_slice(), b"hello 1");
        assert!(recv_msg.handles.is_empty());

        let recv_msg = channel0.read().unwrap();
        assert_eq!(recv_msg.data.as_slice(), b"hello 0");
        assert!(recv_msg.handles.is_empty());

        // read more message should fail.
        assert_eq!(channel0.read().err(), Some(ZxError::SHOULD_WAIT));
        assert_eq!(channel1.read().err(), Some(ZxError::SHOULD_WAIT));
    }

    #[test]
    fn peer_closed() {
        let (channel0, channel1) = Channel::create();
        // write a message from peer, then drop it
        channel1.write(MessagePacket::default()).unwrap();
        drop(channel1);
        // read the first message should success.
        channel0.read().unwrap();
        // read more message should fail.
        assert_eq!(channel0.read().err(), Some(ZxError::PEER_CLOSED));
        // write message should fail.
        assert_eq!(
            channel0.write(MessagePacket::default()),
            Err(ZxError::PEER_CLOSED)
        );
    }

    #[test]
    fn signal() {
        let (channel0, channel1) = Channel::create();

        // initial status is writable and not readable.
        let init_signal = channel0.base.signal();
        assert!(!init_signal.contains(Signal::READABLE));
        assert!(init_signal.contains(Signal::WRITABLE));

        // register callback for `Signal::READABLE` & `Signal::PEER_CLOSED`:
        //   set `readable` and `peer_closed`
        let readable = Arc::new(AtomicBool::new(false));
        let peer_closed = Arc::new(AtomicBool::new(false));
        channel0.add_signal_callback(Box::new({
            let readable = readable.clone();
            let peer_closed = peer_closed.clone();
            move |signal| {
                readable.store(signal.contains(Signal::READABLE), Ordering::SeqCst);
                peer_closed.store(signal.contains(Signal::PEER_CLOSED), Ordering::SeqCst);
                false
            }
        }));

        // writing to peer should trigger `Signal::READABLE`.
        channel1.write(MessagePacket::default()).unwrap();
        assert!(readable.load(Ordering::SeqCst));

        // reading all messages should cause `Signal::READABLE` be cleared.
        channel0.read().unwrap();
        assert!(!readable.load(Ordering::SeqCst));

        // peer closed should trigger `Signal::PEER_CLOSED`.
        assert!(!peer_closed.load(Ordering::SeqCst));
        drop(channel1);
        assert!(peer_closed.load(Ordering::SeqCst));
    }
}
