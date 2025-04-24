use std::{ffi::c_int, io, os::fd::{AsRawFd, RawFd}};
use tokio::io::unix::AsyncFd;

pub struct SockOpts<'opt> {
    /// The ethernet protocol type to bind this socket to. [`libc::ETH_P_ALL`] for example 
    /// would allow reading and writing all arbitrary packet types
    protocol: c_int,
    /// The name of the interface to bind this raw socket to
    intf: &'opt str,
}

pub struct RawSock {
    fd: AsyncFd<RawFd>,
}

impl RawSock {
    pub fn new(opts: SockOpts) -> Result<Self, io::Error> {
        unsafe {
            if opts.intf.len() >= libc::IFNAMSIZ {
                return Err(io::Error::other("invalid interface name - exceeds length"));
            }

            let sock_fd = libc::socket(
                libc::AF_PACKET,
                libc::SOCK_RAW | libc::SOCK_NONBLOCK,
                opts.protocol
            );

            if sock_fd < 0 {
                return Err(io::Error::last_os_error())
            }

            let mut ifreq = libc::ifreq {
                ifr_name: [0;libc::IFNAMSIZ],
                ifr_ifru: std::mem::zeroed(),
            };

            let intf_c = &*(opts.intf.as_bytes() as *const _ as *const [i8]);
            ifreq.ifr_name[..intf_c.len()].copy_from_slice(intf_c);
            
            if libc::ioctl(
                sock_fd,
                libc::SIOCGIFINDEX,
                &ifreq as *const _,
            ) < 0 {
                return Err(io::Error::last_os_error())
            }
        
            let addr = libc::sockaddr_ll {
                sll_family: libc::AF_PACKET as u16,
                sll_protocol: u16::to_be(opts.protocol as u16),
                sll_ifindex: ifreq.ifr_ifru.ifru_ifindex,
                sll_hatype: 0,
                sll_pkttype: 0,
                sll_halen: 0,
                sll_addr: [0; 8],
            };
            
            if libc::bind(sock_fd, &addr as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_ll>() as u32) < 0 {
                return Err(io::Error::last_os_error())
            }

            Ok(Self {
                fd: AsyncFd::new(sock_fd).unwrap(),
            })
        }
    }

    pub async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let guard = self.fd.readable().await?;

            unsafe {
                let res = libc::recv(
                    guard.get_ref().as_raw_fd(),
                    buf as *mut _ as *mut libc::c_void,
                    buf.len(), 
                    0
                );

                if res < 0 {
                    let err = io::Error::last_os_error();

                    match err.kind() {
                        io::ErrorKind::WouldBlock => continue,
                        _ => return Err(err)
                    }
                } else { 
                    return Ok(res as usize)
                }
            }
        }
    }

    pub async fn write(&self, buf: &[u8]) -> io::Result<usize> {
        loop {
            let guard = self.fd.writable().await?;

            unsafe {
                let res = libc::send(
                    guard.get_ref().as_raw_fd(),
                    buf as *const _ as *const libc::c_void,
                    buf.len(),
                    0,
                );

                if res < 0 {
                    let err = io::Error::last_os_error();

                    match err.kind() {
                        io::ErrorKind::WouldBlock => continue,
                        _ => return Err(err)
                    }
                } else { 
                    return Ok(res as usize)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_creation() {
        let my_sock = RawSock::new(SockOpts { protocol: libc::ETH_P_ALL, intf: "lo" }).unwrap();

        let mut my_buf = [0u8;128];

        // ICMP localhost -> localhost
        let packet: &[u8] = &[
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x86, 0xdd, 0x60, 0x04, 0x90, 0x15, 0x00, 0x40, 0x3a, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x80, 0x00, 0xd0, 0x40, 0x00, 0x0a, 0x00, 0x01, 0xb9, 0xb1, 0x09, 0x68, 0x00, 0x00, 0x00, 0x00, 0x27, 0x4b, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37,
        ];

        my_sock.write(&packet).await.unwrap();
        let read_size = my_sock.read(&mut my_buf).await.unwrap();

        assert_eq!(read_size, packet.len());
        assert_eq!(&my_buf[..read_size], packet);
    }
}
