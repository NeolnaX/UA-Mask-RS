use std::net::{SocketAddr, Ipv4Addr};
use std::os::fd::BorrowedFd;
use std::os::unix::io::AsRawFd;

use nix::sys::socket::{getsockopt, sockopt::OriginalDst};
use tokio::net::TcpStream;

pub async fn get_original_dst(stream: &TcpStream) -> Result<SocketAddr, Box<dyn std::error::Error>> {
    let fd = stream.as_raw_fd();
    let borrowed_fd = unsafe { BorrowedFd::borrow_raw(fd) };

    let addr = getsockopt(&borrowed_fd, OriginalDst)
        .map_err(|e| format!("getsockopt SO_ORIGINAL_DST failed: {}", e))?;

    let family = addr.sin_family;
    let port = u16::from_be(addr.sin_port);
    let ip = Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr));

    if family != nix::libc::AF_INET as u16 {
        return Err(format!("unexpected address family: {}", family).into());
    }

    Ok(SocketAddr::new(std::net::IpAddr::V4(ip), port))
}
