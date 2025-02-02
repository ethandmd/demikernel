// Copyright (c) Microsoft Corporation.
// Licensed under the MIT license.

use crate::{
    inetstack::{
        protocols::{
            ethernet2::{
                EtherType2,
                Ethernet2Header,
            },
            ipv4::Ipv4Header,
            tcp::{
                operations::{
                    AcceptFuture,
                    ConnectFuture,
                },
                segment::{
                    TcpHeader,
                    TcpSegment,
                },
                SeqNumber,
            },
        },
        test_helpers::{
            self,
            Engine,
        },
    },
    runtime::{
        memory::DemiBuffer,
        network::{
            consts::RECEIVE_BATCH_SIZE,
            types::MacAddress,
            PacketBuf,
        },
        QDesc,
    },
};
use ::anyhow::Result;
use ::futures::task::noop_waker_ref;
use ::libc::{
    EBADMSG,
    ETIMEDOUT,
};
use ::std::{
    future::Future,
    net::{
        Ipv4Addr,
        SocketAddrV4,
    },
    pin::Pin,
    task::{
        Context,
        Poll,
    },
    time::{
        Duration,
        Instant,
    },
};

//=============================================================================

//tests connection timeout.
#[test]
fn test_connection_timeout() -> Result<()> {
    let mut ctx: Context = Context::from_waker(noop_waker_ref());
    let mut now: Instant = Instant::now();

    // Connection parameters
    let listen_port: u16 = 80;
    let listen_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, listen_port);

    // Setup client.
    let mut client: Engine<RECEIVE_BATCH_SIZE> = test_helpers::new_alice2(now);
    let nretries: usize = client.rt.tcp_config.get_handshake_retries();
    let timeout: Duration = client.rt.tcp_config.get_handshake_timeout();

    // T(0) -> T(1)
    advance_clock(None, Some(&mut client), &mut now);

    // Client: SYN_SENT state at T(1).
    let (_, mut connect_future, bytes): (QDesc, ConnectFuture<RECEIVE_BATCH_SIZE>, DemiBuffer) =
        connection_setup_listen_syn_sent(&mut client, listen_addr)?;

    // Sanity check packet.
    check_packet_pure_syn(
        bytes.clone(),
        test_helpers::ALICE_MAC,
        test_helpers::BOB_MAC,
        test_helpers::ALICE_IPV4,
        test_helpers::BOB_IPV4,
        listen_port,
    )?;

    for _ in 0..nretries {
        for _ in 0..timeout.as_secs() {
            advance_clock(None, Some(&mut client), &mut now);
        }
        client.rt.poll_scheduler();
    }

    match Future::poll(Pin::new(&mut connect_future), &mut ctx) {
        Poll::Ready(Err(error)) if error.errno == ETIMEDOUT => Ok(()),
        _ => anyhow::bail!("connect should have timed out"),
    }
}

//=============================================================================

/// Refuse a connection.
#[test]
fn test_refuse_connection_early_rst() -> Result<()> {
    let _ctx = Context::from_waker(noop_waker_ref());
    let mut now = Instant::now();

    // Connection parameters
    let listen_port: u16 = 80;
    let listen_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, listen_port);

    // Setup peers.
    let mut server: Engine<RECEIVE_BATCH_SIZE> = test_helpers::new_bob2(now);
    let mut client: Engine<RECEIVE_BATCH_SIZE> = test_helpers::new_alice2(now);

    // Server: LISTEN state at T(0).
    let _: AcceptFuture<RECEIVE_BATCH_SIZE> = connection_setup_closed_listen(&mut server, listen_addr)?;

    // T(0) -> T(1)
    advance_clock(Some(&mut server), Some(&mut client), &mut now);

    // Client: SYN_SENT state at T(1).
    let (_, _, bytes): (QDesc, ConnectFuture<RECEIVE_BATCH_SIZE>, DemiBuffer) =
        connection_setup_listen_syn_sent(&mut client, listen_addr)?;

    // Temper packet.
    let (eth2_header, ipv4_header, tcp_header): (Ethernet2Header, Ipv4Header, TcpHeader) =
        extract_headers(bytes.clone())?;
    let segment: TcpSegment = TcpSegment {
        ethernet2_hdr: eth2_header,
        ipv4_hdr: ipv4_header,
        tcp_hdr: TcpHeader {
            src_port: tcp_header.src_port,
            dst_port: tcp_header.dst_port,
            seq_num: tcp_header.seq_num,
            ack_num: tcp_header.ack_num,
            ns: tcp_header.ns,
            cwr: tcp_header.cwr,
            ece: tcp_header.ece,
            urg: tcp_header.urg,
            ack: tcp_header.ack,
            psh: tcp_header.psh,
            rst: true,
            syn: tcp_header.syn,
            fin: tcp_header.fin,
            window_size: tcp_header.window_size,
            urgent_pointer: tcp_header.urgent_pointer,
            num_options: tcp_header.num_options,
            option_list: tcp_header.option_list,
        },
        data: None,
        tx_checksum_offload: false,
    };

    // Serialize segment.
    let buf: DemiBuffer = serialize_segment(segment)?;

    // T(1) -> T(2)
    advance_clock(Some(&mut server), Some(&mut client), &mut now);

    // Server: SYN_RCVD state at T(2).
    match server.receive(buf) {
        Err(error) if error.errno == EBADMSG => Ok(()),
        _ => anyhow::bail!("server receive should have returned an error"),
    }
}

//=============================================================================

/// Refuse a connection.
#[test]
fn test_refuse_connection_early_ack() -> Result<()> {
    let _ctx = Context::from_waker(noop_waker_ref());
    let mut now = Instant::now();

    // Connection parameters
    let listen_port: u16 = 80;
    let listen_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, listen_port);

    // Setup peers.
    let mut server: Engine<RECEIVE_BATCH_SIZE> = test_helpers::new_bob2(now);
    let mut client: Engine<RECEIVE_BATCH_SIZE> = test_helpers::new_alice2(now);

    // Server: LISTEN state at T(0).
    let _: AcceptFuture<RECEIVE_BATCH_SIZE> = connection_setup_closed_listen(&mut server, listen_addr)?;

    // T(0) -> T(1)
    advance_clock(Some(&mut server), Some(&mut client), &mut now);

    // Client: SYN_SENT state at T(1).
    let (_, _, bytes): (QDesc, ConnectFuture<RECEIVE_BATCH_SIZE>, DemiBuffer) =
        connection_setup_listen_syn_sent(&mut client, listen_addr)?;

    // Temper packet.
    let (eth2_header, ipv4_header, tcp_header): (Ethernet2Header, Ipv4Header, TcpHeader) =
        extract_headers(bytes.clone())?;
    let segment: TcpSegment = TcpSegment {
        ethernet2_hdr: eth2_header,
        ipv4_hdr: ipv4_header,
        tcp_hdr: TcpHeader {
            src_port: tcp_header.src_port,
            dst_port: tcp_header.dst_port,
            seq_num: tcp_header.seq_num,
            ack_num: tcp_header.ack_num,
            ns: tcp_header.ns,
            cwr: tcp_header.cwr,
            ece: tcp_header.ece,
            urg: tcp_header.urg,
            ack: true,
            psh: tcp_header.psh,
            rst: tcp_header.rst,
            syn: tcp_header.syn,
            fin: tcp_header.fin,
            window_size: tcp_header.window_size,
            urgent_pointer: tcp_header.urgent_pointer,
            num_options: tcp_header.num_options,
            option_list: tcp_header.option_list,
        },
        data: None,
        tx_checksum_offload: false,
    };

    // Serialize segment.
    let buf: DemiBuffer = serialize_segment(segment)?;

    // T(1) -> T(2)
    advance_clock(Some(&mut server), Some(&mut client), &mut now);

    // Server: SYN_RCVD state at T(2).
    match server.receive(buf) {
        Err(error) if error.errno == EBADMSG => Ok(()),
        _ => anyhow::bail!("server receive should have returned an error"),
    }
}

//=============================================================================

/// Tests connection refuse due to missing syn.
#[test]
fn test_refuse_connection_missing_syn() -> Result<()> {
    let _ctx = Context::from_waker(noop_waker_ref());
    let mut now = Instant::now();

    // Connection parameters
    let listen_port: u16 = 80;
    let listen_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, listen_port);

    // Setup peers.
    let mut server: Engine<RECEIVE_BATCH_SIZE> = test_helpers::new_bob2(now);
    let mut client: Engine<RECEIVE_BATCH_SIZE> = test_helpers::new_alice2(now);

    // Server: LISTEN state at T(0).
    let _: AcceptFuture<RECEIVE_BATCH_SIZE> = connection_setup_closed_listen(&mut server, listen_addr)?;

    // T(0) -> T(1)
    advance_clock(Some(&mut server), Some(&mut client), &mut now);

    // Client: SYN_SENT state at T(1).
    let (_, _, bytes): (QDesc, ConnectFuture<RECEIVE_BATCH_SIZE>, DemiBuffer) =
        connection_setup_listen_syn_sent(&mut client, listen_addr)?;

    // Sanity check packet.
    check_packet_pure_syn(
        bytes.clone(),
        test_helpers::ALICE_MAC,
        test_helpers::BOB_MAC,
        test_helpers::ALICE_IPV4,
        test_helpers::BOB_IPV4,
        listen_port,
    )?;

    // Temper packet.
    let (eth2_header, ipv4_header, tcp_header): (Ethernet2Header, Ipv4Header, TcpHeader) =
        extract_headers(bytes.clone())?;
    let segment: TcpSegment = TcpSegment {
        ethernet2_hdr: eth2_header,
        ipv4_hdr: ipv4_header,
        tcp_hdr: TcpHeader {
            src_port: tcp_header.src_port,
            dst_port: tcp_header.dst_port,
            seq_num: tcp_header.seq_num,
            ack_num: tcp_header.ack_num,
            ns: tcp_header.ns,
            cwr: tcp_header.cwr,
            ece: tcp_header.ece,
            urg: tcp_header.urg,
            ack: tcp_header.ack,
            psh: tcp_header.psh,
            rst: tcp_header.rst,
            syn: false,
            fin: tcp_header.fin,
            window_size: tcp_header.window_size,
            urgent_pointer: tcp_header.urgent_pointer,
            num_options: tcp_header.num_options,
            option_list: tcp_header.option_list,
        },
        data: None,
        tx_checksum_offload: false,
    };

    // Serialize segment.
    let buf: DemiBuffer = serialize_segment(segment)?;

    // T(1) -> T(2)
    advance_clock(Some(&mut server), Some(&mut client), &mut now);

    // Server: SYN_RCVD state at T(2).
    match server.receive(buf) {
        Err(error) if error.errno == EBADMSG => Ok(()),
        _ => anyhow::bail!("server receive should have returned an error"),
    }
}

//=============================================================================

/// Extracts headers of a TCP packet.
fn extract_headers(bytes: DemiBuffer) -> Result<(Ethernet2Header, Ipv4Header, TcpHeader)> {
    let (eth2_header, eth2_payload) = Ethernet2Header::parse(bytes)?;
    let (ipv4_header, ipv4_payload) = Ipv4Header::parse(eth2_payload)?;
    let (tcp_header, _) = TcpHeader::parse(&ipv4_header, ipv4_payload, false)?;

    return Ok((eth2_header, ipv4_header, tcp_header));
}

//=============================================================================

/// Serializes a TCP segment.
fn serialize_segment(pkt: TcpSegment) -> Result<DemiBuffer> {
    let header_size: usize = pkt.header_size();
    let body_size: usize = pkt.body_size();
    let mut buf = DemiBuffer::new((header_size + body_size) as u16);
    pkt.write_header(&mut buf[..header_size]);
    if let Some(body) = pkt.take_body() {
        buf[header_size..].copy_from_slice(&body[..]);
    }
    Ok(buf)
}

//=============================================================================

/// Triggers LISTEN -> SYN_SENT state transition.
fn connection_setup_listen_syn_sent<const N: usize>(
    client: &mut Engine<N>,
    listen_addr: SocketAddrV4,
) -> Result<(QDesc, ConnectFuture<N>, DemiBuffer)> {
    // Issue CONNECT operation.
    let client_fd: QDesc = match client.tcp_socket() {
        Ok(fd) => fd,
        Err(e) => anyhow::bail!("client tcp socket returned error: {:?}", e),
    };
    let connect_future: ConnectFuture<N> = client.tcp_connect(client_fd, listen_addr);

    // SYN_SENT state.
    client.rt.poll_scheduler();
    let bytes: DemiBuffer = client.rt.pop_frame();

    Ok((client_fd, connect_future, bytes))
}

/// Triggers CLOSED -> LISTEN state transition.
fn connection_setup_closed_listen<const N: usize>(
    server: &mut Engine<N>,
    listen_addr: SocketAddrV4,
) -> Result<AcceptFuture<N>> {
    // Issue ACCEPT operation.
    let socket_fd: QDesc = match server.tcp_socket() {
        Ok(fd) => fd,
        Err(e) => anyhow::bail!("server tcp socket returned error: {:?}", e),
    };
    if let Err(e) = server.tcp_bind(socket_fd, listen_addr) {
        anyhow::bail!("server bind returned an error: {:?}", e);
    }
    if let Err(e) = server.tcp_listen(socket_fd, 1) {
        anyhow::bail!("server listen returned an error: {:?}", e);
    }
    let accept_future: AcceptFuture<N> = server.tcp_accept(socket_fd);

    // LISTEN state.
    server.rt.poll_scheduler();

    Ok(accept_future)
}

/// Triggers LISTEN -> SYN_RCVD state transition.
fn connection_setup_listen_syn_rcvd<const N: usize>(server: &mut Engine<N>, bytes: DemiBuffer) -> Result<DemiBuffer> {
    // SYN_RCVD state.
    server.receive(bytes).unwrap();
    server.rt.poll_scheduler();
    Ok(server.rt.pop_frame())
}

/// Triggers SYN_SENT -> ESTABLISHED state transition.
fn connection_setup_syn_sent_established<const N: usize>(
    client: &mut Engine<N>,
    bytes: DemiBuffer,
) -> Result<DemiBuffer> {
    client.receive(bytes).unwrap();
    client.rt.poll_scheduler();
    Ok(client.rt.pop_frame())
}

/// Triggers SYN_RCVD -> ESTABLISHED state transition.
fn connection_setup_sync_rcvd_established<const N: usize>(server: &mut Engine<N>, bytes: DemiBuffer) -> Result<()> {
    server.receive(bytes).unwrap();
    server.rt.poll_scheduler();
    Ok(())
}

/// Checks for a pure SYN packet. This packet is sent by the sender side (active
/// open peer) when transitioning from the LISTEN to the SYN_SENT state.
fn check_packet_pure_syn(
    bytes: DemiBuffer,
    eth2_src_addr: MacAddress,
    eth2_dst_addr: MacAddress,
    ipv4_src_addr: Ipv4Addr,
    ipv4_dst_addr: Ipv4Addr,
    dst_port: u16,
) -> Result<()> {
    let (eth2_header, eth2_payload) = Ethernet2Header::parse(bytes).unwrap();
    crate::ensure_eq!(eth2_header.src_addr(), eth2_src_addr);
    crate::ensure_eq!(eth2_header.dst_addr(), eth2_dst_addr);
    crate::ensure_eq!(eth2_header.ether_type(), EtherType2::Ipv4);
    let (ipv4_header, ipv4_payload) = Ipv4Header::parse(eth2_payload).unwrap();
    crate::ensure_eq!(ipv4_header.get_src_addr(), ipv4_src_addr);
    crate::ensure_eq!(ipv4_header.get_dest_addr(), ipv4_dst_addr);
    let (tcp_header, _) = TcpHeader::parse(&ipv4_header, ipv4_payload, false).unwrap();
    crate::ensure_eq!(tcp_header.dst_port, dst_port);
    crate::ensure_eq!(tcp_header.seq_num, SeqNumber::from(0));
    crate::ensure_eq!(tcp_header.syn, true);

    Ok(())
}

/// Checks for a SYN+ACK packet. This packet is sent by the receiver side
/// (passive open peer) when transitioning from the LISTEN to the SYN_RCVD state.
fn check_packet_syn_ack(
    bytes: DemiBuffer,
    eth2_src_addr: MacAddress,
    eth2_dst_addr: MacAddress,
    ipv4_src_addr: Ipv4Addr,
    ipv4_dst_addr: Ipv4Addr,
    src_port: u16,
) -> Result<()> {
    let (eth2_header, eth2_payload) = Ethernet2Header::parse(bytes).unwrap();
    crate::ensure_eq!(eth2_header.src_addr(), eth2_src_addr);
    crate::ensure_eq!(eth2_header.dst_addr(), eth2_dst_addr);
    crate::ensure_eq!(eth2_header.ether_type(), EtherType2::Ipv4);
    let (ipv4_header, ipv4_payload) = Ipv4Header::parse(eth2_payload).unwrap();
    crate::ensure_eq!(ipv4_header.get_src_addr(), ipv4_src_addr);
    crate::ensure_eq!(ipv4_header.get_dest_addr(), ipv4_dst_addr);
    let (tcp_header, _) = TcpHeader::parse(&ipv4_header, ipv4_payload, false).unwrap();
    crate::ensure_eq!(tcp_header.src_port, src_port);
    crate::ensure_eq!(tcp_header.ack_num, SeqNumber::from(1));
    crate::ensure_eq!(tcp_header.seq_num, SeqNumber::from(0));
    crate::ensure_eq!(tcp_header.syn, true);
    crate::ensure_eq!(tcp_header.ack, true);

    Ok(())
}

/// Checks for a pure ACK on a SYN+ACK packet. This packet is sent by the sender
/// side (active open peer) when transitioning from the SYN_SENT state to the
/// ESTABLISHED state.
fn check_packet_pure_ack_on_syn_ack(
    bytes: DemiBuffer,
    eth2_src_addr: MacAddress,
    eth2_dst_addr: MacAddress,
    ipv4_src_addr: Ipv4Addr,
    ipv4_dst_addr: Ipv4Addr,
    dst_port: u16,
) -> Result<()> {
    let (eth2_header, eth2_payload) = Ethernet2Header::parse(bytes).unwrap();
    crate::ensure_eq!(eth2_header.src_addr(), eth2_src_addr);
    crate::ensure_eq!(eth2_header.dst_addr(), eth2_dst_addr);
    crate::ensure_eq!(eth2_header.ether_type(), EtherType2::Ipv4);
    let (ipv4_header, ipv4_payload) = Ipv4Header::parse(eth2_payload).unwrap();
    crate::ensure_eq!(ipv4_header.get_src_addr(), ipv4_src_addr);
    crate::ensure_eq!(ipv4_header.get_dest_addr(), ipv4_dst_addr);
    let (tcp_header, _) = TcpHeader::parse(&ipv4_header, ipv4_payload, false).unwrap();
    crate::ensure_eq!(tcp_header.dst_port, dst_port);
    crate::ensure_eq!(tcp_header.seq_num, SeqNumber::from(1));
    crate::ensure_eq!(tcp_header.ack_num, SeqNumber::from(1));
    crate::ensure_eq!(tcp_header.ack, true);

    Ok(())
}

/// Advances clock by one second.
pub fn advance_clock<const N: usize>(
    server: Option<&mut Engine<N>>,
    client: Option<&mut Engine<N>>,
    now: &mut Instant,
) {
    *now += Duration::from_secs(1);
    if let Some(server) = server {
        server.clock.advance_clock(*now);
    }
    if let Some(client) = client {
        client.clock.advance_clock(*now);
    }
}

/// Runs 3-way connection setup.
pub fn connection_setup<const N: usize>(
    ctx: &mut Context,
    now: &mut Instant,
    server: &mut Engine<N>,
    client: &mut Engine<N>,
    listen_port: u16,
    listen_addr: SocketAddrV4,
) -> Result<((QDesc, SocketAddrV4), QDesc)> {
    // Server: LISTEN state at T(0).
    let mut accept_future: AcceptFuture<N> = connection_setup_closed_listen(server, listen_addr)?;

    // T(0) -> T(1)
    advance_clock(Some(server), Some(client), now);

    // Client: SYN_SENT state at T(1).
    let (client_fd, mut connect_future, mut bytes): (QDesc, ConnectFuture<N>, DemiBuffer) =
        connection_setup_listen_syn_sent(client, listen_addr)?;

    // Sanity check packet.
    check_packet_pure_syn(
        bytes.clone(),
        test_helpers::ALICE_MAC,
        test_helpers::BOB_MAC,
        test_helpers::ALICE_IPV4,
        test_helpers::BOB_IPV4,
        listen_port,
    )?;

    // T(1) -> T(2)
    advance_clock(Some(server), Some(client), now);

    // Server: SYN_RCVD state at T(2).
    bytes = connection_setup_listen_syn_rcvd(server, bytes)?;

    // Sanity check packet.
    check_packet_syn_ack(
        bytes.clone(),
        test_helpers::BOB_MAC,
        test_helpers::ALICE_MAC,
        test_helpers::BOB_IPV4,
        test_helpers::ALICE_IPV4,
        listen_port,
    )?;

    // T(2) -> T(3)
    advance_clock(Some(server), Some(client), now);

    // Client: ESTABLISHED at T(3).
    bytes = connection_setup_syn_sent_established(client, bytes)?;

    // Sanity check sent packet.
    check_packet_pure_ack_on_syn_ack(
        bytes.clone(),
        test_helpers::ALICE_MAC,
        test_helpers::BOB_MAC,
        test_helpers::ALICE_IPV4,
        test_helpers::BOB_IPV4,
        listen_port,
    )?;
    // T(3) -> T(4)
    advance_clock(Some(server), Some(client), now);

    // Server: ESTABLISHED at T(4).
    connection_setup_sync_rcvd_established(server, bytes)?;

    let (server_fd, addr) = match Future::poll(Pin::new(&mut accept_future), ctx) {
        Poll::Ready(Ok(server_fd)) => server_fd,
        _ => anyhow::bail!("accept should have completed"),
    };
    match Future::poll(Pin::new(&mut connect_future), ctx) {
        Poll::Ready(Ok(())) => {},
        _ => anyhow::bail!("connect should have completed"),
    };

    Ok(((server_fd, addr), client_fd))
}

/// Tests basic 3-way connection setup.
#[test]
fn test_good_connect() -> Result<()> {
    let mut ctx = Context::from_waker(noop_waker_ref());
    let mut now = Instant::now();

    // Connection parameters
    let listen_port: u16 = 80;
    let listen_addr: SocketAddrV4 = SocketAddrV4::new(test_helpers::BOB_IPV4, listen_port);

    // Setup peers.
    let mut server: Engine<RECEIVE_BATCH_SIZE> = test_helpers::new_bob2(now);
    let mut client: Engine<RECEIVE_BATCH_SIZE> = test_helpers::new_alice2(now);

    let ((_, _), _): ((QDesc, SocketAddrV4), QDesc) =
        connection_setup(&mut ctx, &mut now, &mut server, &mut client, listen_port, listen_addr)?;

    Ok(())
}
