use rustls::ClientCertVerified;
use serde::de::EnumAccess;
use tokio::{io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadHalf, WriteHalf}, time};
use tokio::net::{TcpStream, UdpSocket};
use tokio_rustls::{client::TlsStream};
use std::sync::Arc;
use tokio::sync::Mutex;

mod raw;

macro_rules! bind_message {
    (enum $name:ident {$($variant:ident ($field:ty) [$tag:path]),*}) => {
	#[derive(Debug)]
	pub enum $name {
	    $(
		$variant($field),
	    )*
	}

	impl $name {
	    fn tag(&self) -> PacketKind {
		match &self {
		    $(
			Self::$variant(_) => $tag
		    ),*
		}
	    }

	    async fn read_from<R: AsyncRead + Unpin>(r: &mut R) -> Option<$name> {
		let tag = PacketKind::from_tag(r.read_u16().await.ok()?)?;
		dbg!(&tag);
		let len = r.read_u32().await.ok()?;
		match tag {
		    $(
			$tag => {
			    let mut buf = vec![0u8; len as usize];
			    r.read_exact(&mut buf).await.ok()?;
			    let variant = <$field as prost::Message>::decode(buf.as_slice()).ok()?;
			    Some(Self::$variant(variant))
			}
		    ),*
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
		}
		w.write_all(&tag.to_be_bytes()).await.ok()?;
		w.write_all(&(buf.len() as u32).to_be_bytes()).await.ok()?;
		w.write_all(&buf).await.ok()?;
		Some(())
	    }
	}
    };
}

bind_message!{
    enum Message {
	Version(raw::Version) [PacketKind::Version],
	UdpTunnel(raw::UdpTunnel) [PacketKind::UdpTunnel],
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
    }
}

pub struct MumbleClient<H> {
    control: ControlChannel<H>,
    voice: Option<VoiceChannel>,
}

impl<H: MessageHandler> MumbleClient<H> {
    async fn send_message(&self, m: Message) -> Option<()> {
	let mut writer = self.control.write_stream.lock().await;
	m.write_to(&mut *writer).await
    }

    async fn recv_message(&self) -> Option<Message> {
	let mut reader = self.control.read_steam.lock().await;
	Message::read_from(&mut *reader).await
    }

    async fn ping_task(client: Arc<Self>) {
	use std::time::Duration;
	use tokio::time::interval;
	let mut interval = interval(Duration::from_secs(15));
	loop {
	    interval.tick().await;
	    client.send_message(Message::Ping(raw::Ping::default())).await;
	}
    }
}

pub struct ControlChannel<H> {
    read_steam: Mutex<ReadHalf<TlsStream<TcpStream>>>,
    write_stream: Mutex<WriteHalf<TlsStream<TcpStream>>>,
    handler: H
}

pub async fn connect<H: MessageHandler>(host: &str, port: u16, h: H) -> Option<Arc<MumbleClient<H>>>
    where H: Send + Sync + 'static
{
    use tokio_rustls::TlsConnector;
    use webpki::DNSNameRef;
    
    let stream = tokio::net::TcpStream::connect((host, port)).await.ok()?;
    let mut config = rustls::ClientConfig::new();
    config
	.dangerous()
	.set_certificate_verifier(Arc::new(NoCertVerify{}));
    let config = TlsConnector::from(Arc::new(config));
    let host = DNSNameRef::try_from_ascii_str(host).unwrap();
    let stream = config.connect(host, stream).await.ok()?;
    let (r,w) = tokio::io::split(stream);
    let channel = ControlChannel {
	read_steam: Mutex::new(r),
	write_stream: Mutex::new(w),
	handler: h
    };
    let client = Arc::new(MumbleClient {
	control: channel,
	voice: None
    });
    let v = if let Message::Version(v) = client.recv_message().await? {
	v
    } else {
	unimplemented!();
    };
    eprintln!("GOT VERSION - OK!");
    let my_version = Message::Version(raw::Version {
        version: v.version,
        release: Some("1.3.3".to_string()),
        os: Some("gnu/linux".to_string()),
        os_version: None,
    });
    client.send_message(my_version).await?;
    let my_auth = Message::Authenticate(raw::Authenticate {
	username: Some("rumblecat".to_string()),
	password: None,
	tokens: vec![],
	opus: Some(true),
	celt_versions: vec![]
    });
    eprintln!("SENT VERSION - OK!");
    client.send_message(my_auth).await?;
    eprintln!("SENT AUTH - OK!");
    if let Message::CryptSetup(cs) = client.recv_message().await? {
	eprintln!("GOT CRYPTO - OK! {:?}", cs);
    }
    let ping_handle = tokio::task::spawn(MumbleClient::ping_task(Arc::clone(&client)));
    while let Some(msg) = client.recv_message().await {
	eprintln!("GOT MSG = {:?}", msg.tag());
    }
    Some(client)
}

struct NoCertVerify {}
impl rustls::ServerCertVerifier for NoCertVerify {
    fn verify_server_cert(&self, _: &rustls::RootCertStore, _: &[rustls::Certificate], _: webpki::DNSNameRef, _: &[u8]) -> Result<rustls::ServerCertVerified, rustls::TLSError> {
	Ok(rustls::ServerCertVerified::assertion())
    }
}

struct VoiceChannel(UdpSocket);

#[async_trait::async_trait]
pub trait MessageHandler {
    async fn handle_message(m: Message) {}
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
	if tag >= 0 && tag < 26 {
	    unsafe { Some(std::mem::transmute(tag)) }
	} else {
	    None
	}
    }
}

enum VoicePacket<A> {
    Ping {
	timestamp: u64,
    },
    Audio(A)
}

impl VoicePacket<IncomingAudioPacket> {
    async fn read_from<R: AsyncRead + Unpin>(r: &mut R) -> Option<Self> {
	let header = r.read_u8().await.ok()?;
	match (header >> 5) {
	    0b000 => {
		// celt
		let target = Target::from_header_byte(header);
	    },
	    0b001 => {
		//ping
		
	    },
	    0b010 => {
		// speex
		let target = Target::from_header_byte(header);
	    },
	    0b011 => {
		// opus
		let target = Target::from_header_byte(header);
	    },
	    _ => ()
	}
	None
    }
}

async fn varint<R: AsyncRead + Unpin>(r: &mut R) -> Option<u64> {
    let byte = r.read_u8().await.ok()?;
    if byte & 0b10000000 != 0 {
	return Some(byte as u64)
    } else if byte >> 6 == 0b10 {
	// one more byte
    } else if byte >> 5 == 0b110 {
	// two more bytes
    } else if byte >> 4 == 0b1110 {
	// 3 more bytes
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
    None
}

impl VoicePacket<OutgoingAudioPacket> {
    async fn write_to<W: AsyncWrite + Unpin>(w: &mut W) -> Option<()> {
	None
    }
}

struct IncomingAudioPacket {
    target: Target,
    kind: AudioPacketKind,
    session_id: u64,
    sequence_number: u64,
    position_info: Option<(f32, f32, f32)>
}

struct OutgoingAudioPacket {
    target: Target,
    kind: AudioPacketKind,
    sequence_number: u64,
    position_info: Option<(f32, f32, f32)>
}

enum AudioPacketKind {
    CeltAlpha(Vec<u8>),
    CeltBeta(Vec<u8>),
    Speex(Vec<u8>),
    Opus(bool, Vec<u8>)
}

#[derive(Debug)]
enum Target {
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
