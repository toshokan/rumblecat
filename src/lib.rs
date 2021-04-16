use tokio::io::AsyncReadExt;
use tokio::net::{TcpStream, UdpSocket};
use std::sync::Arc;

mod raw;

pub struct MumbleClient {
    control: ControlChannel,
    voice: VoiceChannel,
}

struct ControlChannel {
    stream: TcpStream,
    handler: Arc<dyn MessageHandler>
}

impl ControlChannel {
    async fn receive(&mut self) -> Option<()> {
	macro_rules! dispatch_message {
	    ($tag:ident, $len:ident, $stream:ident, [$(($variant:pat, $p:ty, $func:path)),*]) => {{
		use prost::Message;
		
		match $tag {
		    $(
			$variant => {
			    let mut buf = vec![0u8; $len as usize];
			    $stream.read_exact(&mut buf).await.ok()?;
			    let p = <$p>::decode(buf.as_slice()).ok()?;
			    $func(&*self.handler, p);
			}
		    )*,
		    _ => unimplemented!()
		}
	    }};
	}
	
	let stream = &mut self.stream;
	loop {
	    let tag = PacketKind::from_tag(stream.read_u16().await.ok()?)?;
	    let len = stream.read_u32().await.ok()?;
	    dispatch_message!(tag, len, stream, [
		(PacketKind::Version, raw::Version, MessageHandler::version),
		(PacketKind::UdpTunnel, raw::UdpTunnel, MessageHandler::udp_tunnel),
		(PacketKind::Authenticate, raw::Authenticate, MessageHandler::authenticate),
		(PacketKind::Ping, raw::Ping, MessageHandler::ping),
		(PacketKind::Reject, raw::Reject, MessageHandler::reject),
		(PacketKind::ServerSync, raw::ServerSync, MessageHandler::server_sync),
		(PacketKind::ChannelRemove, raw::ChannelRemove, MessageHandler::channel_remove),
		(PacketKind::UserRemove, raw::UserRemove, MessageHandler::user_remove),
		(PacketKind::UserState, raw::UserState, MessageHandler::user_state),
		(PacketKind::BanList, raw::BanList, MessageHandler::ban_list),
		(PacketKind::TextMessage, raw::TextMessage, MessageHandler::text_message),
		(PacketKind::PermissionDenied, raw::PermissionDenied, MessageHandler::permission_denied),
		(PacketKind::Acl, raw::Acl, MessageHandler::acl),
		(PacketKind::QueryUsers, raw::QueryUsers, MessageHandler::query_users),
		(PacketKind::CryptSetup, raw::CryptSetup, MessageHandler::crypt_setup),
		(PacketKind::ContextActionModify, raw::ContextActionModify, MessageHandler::context_action_modify),
		(PacketKind::ContextAction, raw::ContextAction, MessageHandler::context_action),
		(PacketKind::UserList, raw::UserList, MessageHandler::user_list),
		(PacketKind::VoiceTarget, raw::VoiceTarget, MessageHandler::voice_target),
		(PacketKind::PermissionQuery, raw::PermissionQuery, MessageHandler::permission_query),
		(PacketKind::CodecVersion, raw::CodecVersion, MessageHandler::codec_version),
		(PacketKind::UserStats, raw::UserStats, MessageHandler::user_stats),
		(PacketKind::RequestBlob, raw::RequestBlob, MessageHandler::request_blob),
		(PacketKind::ServerConfig, raw::ServerConfig, MessageHandler::server_config),
		(PacketKind::SuggestConfig, raw::SuggestConfig, MessageHandler::suggest_config)
	    ])
	}
    }
}

struct VoiceChannel(UdpSocket);

macro_rules! impl_fns {
    ($($fn_name:ident : $msg:ty),*) => {
	$(
	    fn $fn_name(&self, _msg: $msg) {}
	)*
    };
}


#[async_trait::async_trait]
trait MessageHandler {
    impl_fns!{
	version : raw::Version,
	udp_tunnel : raw::UdpTunnel,
	authenticate : raw::Authenticate,
	ping : raw::Ping,
	reject : raw::Reject,
	server_sync : raw::ServerSync,
	channel_remove : raw::ChannelRemove,
	user_remove : raw::UserRemove,
	user_state : raw::UserState,
	ban_list : raw::BanList,
	text_message : raw::TextMessage,
	permission_denied : raw::PermissionDenied,
	acl : raw::Acl,
	query_users : raw::QueryUsers,
	crypt_setup : raw::CryptSetup,
	context_action_modify : raw::ContextActionModify,
	context_action : raw::ContextAction,
	user_list : raw::UserList,
	voice_target : raw::VoiceTarget,
	permission_query : raw::PermissionQuery,
	codec_version : raw::CodecVersion,
	user_stats : raw::UserStats,
	request_blob : raw::RequestBlob,
	server_config : raw::ServerConfig,
	suggest_config : raw::SuggestConfig
    }
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
