use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio_rustls::client::TlsStream;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{UnboundedSender, UnboundedReceiver};
use futures::{SinkExt, stream::StreamExt};

mod raw;

macro_rules! bind_message {
    (enum ($iname:ident, $oname:ident) {$($variant:ident ($field:ty) [$tag:path]),* | $($vvariant:ident ($vfield:ty, $vfield2:ty) [$vtag:path]),*}) => {
	#[derive(Debug)]
	pub enum $iname {
	    $(
		$variant($field),
	    )*
	    $(
		$vvariant($vfield),
	    )*
	}

	#[derive(Debug)]
	pub enum $oname {
	    $(
		$variant($field),
	    )*
	    $(
	        $vvariant($vfield2),
	    )*
	}

	impl $iname {
	    fn tag(&self) -> PacketKind {
		match &self {
		    $(
			Self::$variant(_) => $tag,
		    )*
		    $(
		        Self::$vvariant(_) => $vtag,
		    )*
		}
	    }

	    async fn read_from<R: AsyncRead + Unpin>(r: &mut R) -> Option<$iname> {
		let tag_bytes = r.read_u16().await.ok()?;
		let tag = PacketKind::from_tag(tag_bytes)?;
		let len = r.read_u32().await.ok()?;
		match tag {
		    $(
			$tag => {
			    let mut buf = vec![0u8; len as usize];
			    r.read_exact(&mut buf).await.ok()?;
			    let variant = <$field as prost::Message>::decode(buf.as_slice()).ok()?;
			    Some(Self::$variant(variant))
			}
		    ),*,
		    $(
			$vtag => {
			    let pkt = <$vfield>::read_from(r).await?;
			    Some(Self::$vvariant(pkt))
			}
		    ),*
		}
	    }
	}

	impl $oname {
	    fn tag(&self) -> PacketKind {
		match &self {
		    $(
			Self::$variant(_) => $tag,
		    )*
		    $(
		        Self::$vvariant(_) => $vtag,
		    )*
		}
	    }
	    
	    async fn write_to<W: AsyncWrite + Unpin>(self, w: &mut W) -> Option<()> {
		use tokio::io::AsyncWriteExt;
		use prost::Message;
		
		let tag = self.tag() as u16;
		let mut buf = vec![];
		match self {
		    $(
			Self::$variant(f) => {
			    f.encode(&mut buf).ok()?
			}
		    ),*
		    $(
		        Self::$vvariant(_) => {
		    	    unimplemented!()
		        }
		    ),*
		}
		w.write_all(&tag.to_be_bytes()).await.ok()?;
		w.write_all(&(buf.len() as u32).to_be_bytes()).await.ok()?;
		w.write_all(&buf).await.ok()?;
		Some(())
	    }
	}
    };
}

bind_message! {
    enum (IncomingMessage, OutgoingMessage) {
	Version(raw::Version) [PacketKind::Version],
	Authenticate(raw::Authenticate) [PacketKind::Authenticate],
	Ping(raw::Ping) [PacketKind::Ping],
	Reject(raw::Reject) [PacketKind::Reject],
	ServerSync(raw::ServerSync) [PacketKind::ServerSync],
	ChannelRemove(raw::ChannelRemove) [PacketKind::ChannelRemove],
	ChannelState(raw::ChannelState) [PacketKind::ChannelState],
	UserRemove(raw::UserRemove) [PacketKind::UserRemove],
	UserState(raw::UserState) [PacketKind::UserState],
	BanList(raw::BanList) [PacketKind::BanList],
	TextMessage(raw::TextMessage) [PacketKind::TextMessage],
	PermissionDenied(raw::PermissionDenied) [PacketKind::PermissionDenied],
	Acl(raw::Acl) [PacketKind::Acl],
	QueryUsers(raw::QueryUsers) [PacketKind::QueryUsers],
	CryptSetup(raw::CryptSetup) [PacketKind::CryptSetup],
	ContextActionModify(raw::ContextActionModify) [PacketKind::ContextActionModify],
	ContextAction(raw::ContextAction) [PacketKind::ContextAction],
	UserList(raw::UserList) [PacketKind::UserList],
	VoiceTarget(raw::VoiceTarget) [PacketKind::VoiceTarget],
	PermissionQuery(raw::PermissionQuery) [PacketKind::PermissionQuery],
	CodecVersion(raw::CodecVersion) [PacketKind::CodecVersion],
	UserStats(raw::UserStats) [PacketKind::UserStats],
	RequestBlob(raw::RequestBlob) [PacketKind::RequestBlob],
	ServerConfig(raw::ServerConfig) [PacketKind::ServerConfig],
	SuggestConfig(raw::SuggestConfig) [PacketKind::SuggestConfig]
	    |
	UdpTunnel(VoicePacket<IncomingAudioPacket>, VoicePacket<OutgoingAudioPacket>) [PacketKind::UdpTunnel]
    }
}

pub struct MumbleClient {
    control: Arc<ControlChannel>,
    tasks: MaintenanceTasks,
    pub rx: UnboundedReceiver<IncomingMessage>,
    tx: UnboundedSender<OutgoingMessage>
}

pub struct MaintenanceTasks {
    ping: JoinHandle<()>,
    send: JoinHandle<()>,
    recv: JoinHandle<()>
}

impl MaintenanceTasks {
    async fn ping_task(control: Arc<ControlChannel>) {
	use std::time::Duration;
	use tokio::time::interval;
	
	let mut interval = interval(Duration::from_secs(15));
	loop {
	    interval.tick().await;
	    control.send_message(OutgoingMessage::Ping(raw::Ping::default())).await;
	}
    }

    fn send_task(control: Arc<ControlChannel>) -> (JoinHandle<()>, UnboundedSender<OutgoingMessage>) {
	let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
	let handle = tokio::task::spawn(async move {
	    while let Some(msg) = rx.recv().await {
		control.send_message(msg).await.unwrap()
	    }
	});
	(handle, tx)
    }

    fn recv_task(control: Arc<ControlChannel>) -> (JoinHandle<()>, UnboundedReceiver<IncomingMessage>) {
	let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
	let handle = tokio::task::spawn(async move {
	    while let Some(msg) = control.recv_message().await {
		tx.send(msg);
	    }
	});
	(handle, rx)
    }
    
    fn start(control: Arc<ControlChannel>) -> (Self, UnboundedSender<OutgoingMessage>, UnboundedReceiver<IncomingMessage>) {
	let (send, tx) = Self::send_task(Arc::clone(&control));
	let (recv, rx) = Self::recv_task(Arc::clone(&control));
	(Self {
	    ping: tokio::task::spawn(Self::ping_task(control)),
	    send,
	    recv
	}, tx, rx)
    }
}

impl MumbleClient {
    async fn send_message(&self, m: OutgoingMessage) -> Option<()> {
	self.control.send_message(m).await
    }

    async fn recv_message(&self) -> Option<IncomingMessage> {
	self.control.recv_message().await
    }
}

pub struct ControlChannel {
    read: Mutex<ReadHalf<TlsStream<TcpStream>>>,
    write: Mutex<WriteHalf<TlsStream<TcpStream>>>,
}

impl ControlChannel {
    async fn send_message(&self, m: OutgoingMessage) -> Option<()> {
	let mut writer = self.write.lock().await;
	m.write_to(&mut *writer).await
    }

    async fn recv_message(&self) -> Option<IncomingMessage> {
	let mut reader = self.read.lock().await;
	IncomingMessage::read_from(&mut *reader).await
    }
}

pub struct MumbleConnector {
    host: String,
    port: u16,
    tls_connector: tokio_rustls::TlsConnector,
}

struct NoCertVerify {}
impl rustls::ServerCertVerifier for NoCertVerify {
    fn verify_server_cert(&self,
			  _: &rustls::RootCertStore,
			  _: &[rustls::Certificate],
			  _: webpki::DNSNameRef,
			  _: &[u8]) -> Result<rustls::ServerCertVerified, rustls::TLSError> {
	Ok(rustls::ServerCertVerified::assertion())
    }
}

impl MumbleConnector {
    pub fn new(host: String, port: u16) -> Self {
	use tokio_rustls::TlsConnector;
	
	let mut config = rustls::ClientConfig::new();
	config.dangerous()
	    .set_certificate_verifier(Arc::new(NoCertVerify{}));

	Self {
	    host,
	    port,
	    tls_connector: TlsConnector::from(Arc::new(config))
	}
    }

    pub async fn connect(&self, user: &str) -> Option<MumbleClient> {
	let channel = self.connect_stream().await?;
	let client = Self::handshake(user, channel).await?;
	Some(client)
    }
    
    async fn connect_stream(&self) -> Option<ControlChannel> {
	let stream = tokio::net::TcpStream::connect((self.host.clone(), self.port)).await.ok()?;
	let dns_host = webpki::DNSNameRef::try_from_ascii_str(&self.host).unwrap();
	let stream = self.tls_connector.connect(dns_host, stream).await.ok()?;
	let (r, w) = tokio::io::split(stream);
	Some(ControlChannel {
	    read: Mutex::new(r),
	    write: Mutex::new(w)
	})
    }

    fn my_version(server_version: Option<u32>) -> OutgoingMessage {
	OutgoingMessage::Version(raw::Version {
	    version: server_version,
	    os: Some("GNU/Linux".to_string()),
	    ..Default::default()
	})
    }

    async fn handshake(user: &str, channel: ControlChannel) -> Option<MumbleClient> {
	let server_version = match channel.recv_message().await? {
	    IncomingMessage::Version(v) => v,
	    _ => unimplemented!()
	};
	channel.send_message(Self::my_version(server_version.version)).await?;
	
	let auth = OutgoingMessage::Authenticate(raw::Authenticate {
	    username: Some(user.to_string()),
	    opus: Some(true),
	    ..Default::default()
	});
	channel.send_message(auth).await?;

	if let IncomingMessage::CryptSetup(_) = channel.recv_message().await? {
	    // ignore
	}

	let channel = Arc::new(channel);

	let (tasks, tx, rx) = MaintenanceTasks::start(Arc::clone(&channel));

	Some(MumbleClient {
	    control: channel,
	    tasks,
	    rx,
	    tx
	})
    }
}

#[async_trait::async_trait]
pub trait MessageHandler {
    async fn handle_message(m: IncomingMessage) {}
}

#[repr(u16)]
#[non_exhaustive]
#[derive(Debug)]
pub enum PacketKind {
    Version = 0,
    UdpTunnel = 1,
    Authenticate = 2,
    Ping = 3,
    Reject = 4,
    ServerSync = 5,
    ChannelRemove = 6,
    ChannelState = 7,
    UserRemove = 8,
    UserState = 9,
    BanList = 10,
    TextMessage = 11,
    PermissionDenied = 12,
    Acl = 13,
    QueryUsers = 14,
    CryptSetup = 15,
    ContextActionModify = 16,
    ContextAction = 17,
    UserList = 18,
    VoiceTarget = 19,
    PermissionQuery = 20,
    CodecVersion = 21,
    UserStats = 22,
    RequestBlob = 23,
    ServerConfig = 24,
    SuggestConfig = 25,
}

impl PacketKind {
    fn is_voice(&self) -> bool {
	if let Self::UdpTunnel = self {
	    true
	} else {
	    false
	}
    }
    
    fn from_tag(tag: u16) -> Option<Self> {
	if tag < 26 {
	    unsafe { Some(std::mem::transmute(tag)) }
	} else {
	    None
	}
    }
}

#[derive(Debug)]
pub enum VoicePacket<A> {
    Ping {
	timestamp: u64,
    },
    Audio(A)
}

impl VoicePacket<IncomingAudioPacket> {
    async fn read_from<R: AsyncRead + Unpin>(r: &mut R) -> Option<Self> {
	let header = r.read_u8().await.ok()?;
	match header >> 5 {
	    0b000 => {
		// celt alpha
		let target = Target::from_header_byte(header);
		let session_id = varint(r).await?;
		let sequence_number = varint(r).await?;
		let data = read_speex_or_celt_payload(r).await?;
		Some(VoicePacket::Audio(IncomingAudioPacket {
		    target,
		    kind: AudioPacketKind::CeltAlpha(data),
		    session_id,
		    sequence_number,
		}))
	    },
	    0b001 => {
		//ping
		let timestamp = varint(r).await?;
		Some(VoicePacket::Ping {
		    timestamp
		})
	    },
	    0b010 => {
		// speex
		let target = Target::from_header_byte(header);
		let session_id = varint(r).await?;
		let sequence_number = varint(r).await?;
		let data = read_speex_or_celt_payload(r).await?;
		Some(VoicePacket::Audio(IncomingAudioPacket {
		    target,
		    kind: AudioPacketKind::Speex(data),
		    session_id,
		    sequence_number,
		}))
	    },
	    0b011 => {
		// celt beta
		let target = Target::from_header_byte(header);
		let session_id = varint(r).await?;
		let sequence_number = varint(r).await?;
		let data = read_speex_or_celt_payload(r).await?;
		Some(VoicePacket::Audio(IncomingAudioPacket {
		    target,
		    kind: AudioPacketKind::CeltBeta(data),
		    session_id,
		    sequence_number,
		}))
	    },
	    0b100 => {
		// opus
		let target = Target::from_header_byte(header);
		let session_id = varint(r).await?;
		let sequence_number = varint(r).await?;
		let data = read_opus_payload(r).await?;
		Some(VoicePacket::Audio(IncomingAudioPacket {
		    target,
		    kind: AudioPacketKind::Opus(data),
		    session_id,
		    sequence_number,
		}))
	    },
	    _ => None
	}
    }
}

async fn varint<R: AsyncRead + Unpin>(r: &mut R) -> Option<u64> {
    let byte = r.read_u8().await.ok()?;
    if byte & 0b10000000 == 0 {
	return Some(byte as u64)
    } else if byte >> 6 == 0b10 {
	// one more byte
	let byte = (byte & 0b111111) as u64;
	let rest = r.read_u8().await.ok()? as u64;
	return Some(((byte << 8) | rest) as u64)
    } else if byte >> 5 == 0b110 {
	// two more bytes
	let byte = (byte & 0b11111) as u64;
	let rest = r.read_u16().await.ok()? as u64;
	return Some(((byte << 16) | rest) as u64)
    } else if byte >> 4 == 0b1110 {
	// 3 more bytes
	let byte = (byte & 0b1111) as u64;
	let rest = r.read_u32().await.ok()? as u64;
	return Some(((byte << 32) | rest) as u64)
    } else if byte >> 2 == 0b111100 {
	let int = r.read_u32().await.ok()?;
	return Some(int as u64)
    } else if byte >> 2 == 0b111101 {
	let long = r.read_u64().await.ok()?;
	return Some(long)
    } else if byte >> 2 == 0b111110 {
	// hmm, this breaks the type...
	// let v = variant(r).map(|i| i * -1)
    } else if byte >> 2 == 0b111111 {
	return Some(!(byte & 0b11) as u64)
    }
    unimplemented!()
}

async fn read_speex_or_celt_payload<R: AsyncRead + Unpin>(r: &mut R) -> Option<Vec<AudioFrame>> {
    let mut frames = vec![];
    loop {
	let header = r.read_u8().await.ok()?;
	let len = header & 0b01111111;
	if len == 0 {
	    break;
	}
	let mut buf2 = vec![0u8; len as usize];
	r.read_exact(&mut buf2).await.ok()?;
	if header & 0b10000000 != 0 {
	    frames.push(AudioFrame {
		is_terminal: true,
		data: buf2
	    });
	    break;
	}
	frames.push(AudioFrame {
	    is_terminal: false,
	    data: buf2
	});
    }
    Some(frames)
}

async fn read_opus_payload<R: AsyncRead + Unpin>(r: &mut R) -> Option<AudioFrame> {
    let header = varint(r).await?;
    let is_terminal = header & 0x2000 != 0;
    let len = header & 0x1FFF;
    let mut data = vec![0u8; len as usize];
    r.read_exact(&mut data).await.ok()?;
    Some(AudioFrame {
	is_terminal,
	data
    })
}

impl VoicePacket<OutgoingAudioPacket> {
    async fn write_to<W: AsyncWrite + Unpin>(w: &mut W) -> Option<()> {
	None
    }
}

#[derive(Debug)]
pub struct IncomingAudioPacket {
    target: Target,
    kind: AudioPacketKind,
    session_id: u64,
    sequence_number: u64,
}

#[derive(Debug)]
pub struct OutgoingAudioPacket {
    target: Target,
    kind: AudioPacketKind,
    sequence_number: u64,
}

#[derive(Debug)]
pub enum AudioPacketKind {
    CeltAlpha(Vec<AudioFrame>),
    CeltBeta(Vec<AudioFrame>),
    Speex(Vec<AudioFrame>),
    Opus(AudioFrame)
}

#[derive(Debug)]
pub struct AudioFrame {
    is_terminal: bool,
    data: Vec<u8>
}

#[derive(Debug)]
pub enum Target {
    Normal,
    Whisper(u8),
    ServerLoopback
}

impl Target {
    fn from_header_byte(byte: u8) -> Self {
	let byte = byte & 0b11111;
	match byte {
	    0 => Self::Normal,
	    31 => Self::ServerLoopback,
	    i => Self::Whisper(i)
	}
    }
}

pub async fn serve(connector: MumbleConnector) -> Option<()> {
    use warp::Filter;

    let connector = Arc::new(connector);
    
    let routes = warp::path("rumble")
        .and(warp::any().map(move || connector.clone()))
        .and(warp::path::param())
        .and(warp::ws())
        .map(|connector: Arc<MumbleConnector>, user: String, ws: warp::ws::Ws| {

	    
	    ws.on_upgrade(|socket| async move {
		let (mut tx, mut rx) = socket.split();
		let mut connection = connector.connect(&user).await.unwrap();		
		let handle = tokio::task::spawn(async move {
		    while let Some(msg) = connection.rx.recv().await {
			tx.send(warp::ws::Message::text(format!("got message {:?} from mumble", msg.tag()))).await.unwrap();
		    }
		});
		
		while let Some(x) = rx.next().await {
		    eprintln!("{:?}", x)
		}
	    })
	});

    warp::serve(routes).run(([0, 0, 0, 0], 3030)).await;
    Some(())
}
