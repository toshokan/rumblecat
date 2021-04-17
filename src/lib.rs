use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::net::{TcpStream, UdpSocket};

mod raw;

macro_rules! bind_message {
    (enum $name:ident {$($variant:ident ($field:ty) [$tag:path]),*}) => {
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

	    async fn write_to<W: AsyncWrite + Unpin>(w: &mut W, m: $name) -> Option<()> {
		use tokio::io::AsyncWriteExt;
		use prost::Message;
		
		let tag = m.tag() as u16;
		let mut buf = vec![];
		match m {
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
    voice: VoiceChannel,
}

struct ControlChannel<H> {
    stream: TcpStream,
    handler: H
}

struct VoiceChannel(UdpSocket);

#[async_trait::async_trait]
trait MessageHandler {
    async fn handle_message(m: Message);
}

#[repr(u16)]
#[non_exhaustive]
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
    fn from_tag(tag: u16) -> Option<Self> {
	if tag > 0 && tag < 26 {
	    unsafe { Some(std::mem::transmute(tag)) }
	} else {
	    None
	}
    }
}
