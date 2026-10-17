#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LogModule {
    Main,
    IPC,
    NetPacket,
    Manifest,
    KeyValue,
    DecryptionKey,
    Misc,
    Achievement,
    Pics,
    OnlineFix,
    RichPresence,
    Package,
    SteamUI,
    Inject,
    Pipe,
    Platform,
}

impl LogModule {
    pub const ALL: &'static [LogModule] = &[
        LogModule::Main,
        LogModule::IPC,
        LogModule::NetPacket,
        LogModule::Manifest,
        LogModule::KeyValue,
        LogModule::DecryptionKey,
        LogModule::Misc,
        LogModule::Achievement,
        LogModule::Pics,
        LogModule::OnlineFix,
        LogModule::RichPresence,
        LogModule::Package,
        LogModule::SteamUI,
        LogModule::Inject,
        LogModule::Pipe,
        LogModule::Platform,
    ];

    pub const COUNT: usize = Self::ALL.len();

    pub const fn as_index(self) -> usize { self as usize }

    pub const fn file_name(self) -> &'static str {
        match self {
            LogModule::Main          => "main",
            LogModule::IPC           => "ipc",
            LogModule::NetPacket     => "netpacket",
            LogModule::Manifest      => "manifest",
            LogModule::KeyValue      => "keyvalue",
            LogModule::DecryptionKey => "decryptionkey",
            LogModule::Misc          => "misc",
            LogModule::Achievement   => "achievement",
            LogModule::Pics          => "pics",
            LogModule::OnlineFix     => "onlinefix",
            LogModule::RichPresence  => "richpresence",
            LogModule::Package       => "package",
            LogModule::SteamUI       => "steamui",
            LogModule::Inject        => "inject",
            LogModule::Pipe          => "pipe",
            LogModule::Platform      => "platform",
        }
    }
}
