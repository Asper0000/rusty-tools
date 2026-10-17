//! All steamclient enums. Source: `Steam/Enums.h`
//!
//! Notes on the port:
//!  * `EMsg` is a newtype over `u32`, not a Rust enum, because the C++ enum
//!    contains aliased values (e.g. `k_EMsgBaseGeneral == k_EMsgGenericReply`).
//!    Every named value and alias is preserved as an associated const.
//!    Pattern-match via `msg.0` when you need to switch on a value.
//!  * `EAppOwnershipFlags`, `EAppState` and `EAppChangeFlags` are modelled
//!    with the `bitflags` crate because they are used as bitmasks in C++.
//!  * `EWebSocketOpCode : char` uses `#[repr(u8)]` - all defined values are
//!    non-negative. `char` on MSVC x64 is signed, but no negative values
//!    are used.

#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]

use bitflags::bitflags;
use super::types::*;

#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BillingType {
    NoCost = 0,
    BillOnceOnly = 1,
    BillMonthly = 2,
    ProofOfPrepurchaseOnly = 3,
    GuestPass = 4,
    HardwarePromo = 5,
    Gift = 6,
    AutoGrant = 7,
    OEMTicket = 8,
    RecurringOption = 9,
    BillOnceOrCDKey = 10,
    Repurchaseable = 11,
    FreeOnDemand = 12,
    Rental = 13,
    CommercialLicense = 14,
    FreeCommercialLicense = 15,
    NumBillingTypes = 16,
}

#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ELicenseType {
    NoLicense = 0,
    SinglePurchase = 1,
    SinglePurchaseLimitedUse = 2,
    RecurringCharge = 3,
    RecurringChargeLimitedUse = 4,
    RecurringChargeLimitedUseWithOverages = 5,
    RecurringOption = 6,
    LimitedUseDelayedActivation = 7,
}

#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EPackageStatus {
    Available = 0,
    Preorder = 1,
    Unavailable = 2,
    Invalid = 3,
}

#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EAppReleaseState {
    UnknownApp = 0,
    Unavailable = 1,
    Prerelease = 2,
    PreloadOnly = 3,
    Released = 4,
}

bitflags! {
    /// Source: `enum EAppOwnershipFlags` - used as a bitmask.
    #[repr(transparent)]
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct EAppOwnershipFlags: u32 {
        const None                = 0x0000;
        const OwnsLicense         = 0x0001;
        const FreeLicense         = 0x0002;
        const RegionRestricted    = 0x0004;
        const LowViolence         = 0x0008;
        const InvalidPlatform     = 0x0010;
        const SharedLicense       = 0x0020;
        const FreeWeekend         = 0x0040;
        const RetailLicense       = 0x0080;
        const LicenseLocked       = 0x0100;
        const LicensePending      = 0x0200;
        const LicenseExpired      = 0x0400;
        const LicensePermanent    = 0x0800;
        const LicenseRecurring    = 0x1000;
        const LicenseCanceled     = 0x2000;
        const AutoGrant           = 0x4000;
        const PendingGift         = 0x8000;
        const RentalNotActivated  = 0x0001_0000;
        const Rental              = 0x0002_0000;
        const SiteLicense         = 0x0004_0000;
        const LegacyFreeSub       = 0x0008_0000;
        const InvalidOSType       = 0x0010_0000;
        const TimedTrial          = 0x0020_0000;
    }
}

#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EGameIDType {
    App = 0,
    GameMod = 1,
    Shortcut = 2,
    P2P = 3,
}

/// Error results. Source: `enum EResult` - all ~130 values preserved.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EResult {
    None = 0,
    OK = 1,
    Fail = 2,
    NoConnection = 3,
    InvalidPassword = 5,
    LoggedInElsewhere = 6,
    InvalidProtocolVer = 7,
    InvalidParam = 8,
    FileNotFound = 9,
    Busy = 10,
    InvalidState = 11,
    InvalidName = 12,
    InvalidEmail = 13,
    DuplicateName = 14,
    AccessDenied = 15,
    Timeout = 16,
    Banned = 17,
    AccountNotFound = 18,
    InvalidSteamID = 19,
    ServiceUnavailable = 20,
    NotLoggedOn = 21,
    Pending = 22,
    EncryptionFailure = 23,
    InsufficientPrivilege = 24,
    LimitExceeded = 25,
    Revoked = 26,
    Expired = 27,
    AlreadyRedeemed = 28,
    DuplicateRequest = 29,
    AlreadyOwned = 30,
    IPNotFound = 31,
    PersistFailed = 32,
    LockingFailed = 33,
    LogonSessionReplaced = 34,
    ConnectFailed = 35,
    HandshakeFailed = 36,
    IOFailure = 37,
    RemoteDisconnect = 38,
    ShoppingCartNotFound = 39,
    Blocked = 40,
    Ignored = 41,
    NoMatch = 42,
    AccountDisabled = 43,
    ServiceReadOnly = 44,
    AccountNotFeatured = 45,
    AdministratorOK = 46,
    ContentVersion = 47,
    TryAnotherCM = 48,
    PasswordRequiredToKickSession = 49,
    AlreadyLoggedInElsewhere = 50,
    Suspended = 51,
    Cancelled = 52,
    DataCorruption = 53,
    DiskFull = 54,
    RemoteCallFailed = 55,
    PasswordUnset = 56,
    ExternalAccountUnlinked = 57,
    PSNTicketInvalid = 58,
    ExternalAccountAlreadyLinked = 59,
    RemoteFileConflict = 60,
    IllegalPassword = 61,
    SameAsPreviousValue = 62,
    AccountLogonDenied = 63,
    CannotUseOldPassword = 64,
    InvalidLoginAuthCode = 65,
    AccountLogonDeniedNoMail = 66,
    HardwareNotCapableOfIPT = 67,
    IPTInitError = 68,
    ParentalControlRestricted = 69,
    FacebookQueryError = 70,
    ExpiredLoginAuthCode = 71,
    IPLoginRestrictionFailed = 72,
    AccountLockedDown = 73,
    AccountLogonDeniedVerifiedEmailRequired = 74,
    NoMatchingURL = 75,
    BadResponse = 76,
    RequirePasswordReEntry = 77,
    ValueOutOfRange = 78,
    UnexpectedError = 79,
    Disabled = 80,
    InvalidCEGSubmission = 81,
    RestrictedDevice = 82,
    RegionLocked = 83,
    RateLimitExceeded = 84,
    AccountLoginDeniedNeedTwoFactor = 85,
    ItemDeleted = 86,
    AccountLoginDeniedThrottle = 87,
    TwoFactorCodeMismatch = 88,
    TwoFactorActivationCodeMismatch = 89,
    AccountAssociatedToMultiplePartners = 90,
    NotModified = 91,
    NoMobileDevice = 92,
    TimeNotSynced = 93,
    SmsCodeFailed = 94,
    AccountLimitExceeded = 95,
    AccountActivityLimitExceeded = 96,
    PhoneActivityLimitExceeded = 97,
    RefundToWallet = 98,
    EmailSendFailure = 99,
    NotSettled = 100,
    NeedCaptcha = 101,
    GSLTDenied = 102,
    GSOwnerDenied = 103,
    InvalidItemType = 104,
    IPBanned = 105,
    GSLTExpired = 106,
    InsufficientFunds = 107,
    TooManyPending = 108,
    NoSiteLicensesFound = 109,
    WGNetworkSendExceeded = 110,
    AccountNotFriends = 111,
    LimitedUserAccount = 112,
    CantRemoveItem = 113,
    AccountDeleted = 114,
    ExistingUserCancelledLicense = 115,
    CommunityCooldown = 116,
    NoLauncherSpecified = 117,
    MustAgreeToSSA = 118,
    LauncherMigrated = 119,
    SteamRealmMismatch = 120,
    InvalidSignature = 121,
    ParseFailure = 122,
    NoVerifiedPhone = 123,
    InsufficientBattery = 124,
    ChargerRequired = 125,
    CachedCredentialInvalid = 126,
    PhoneNumberIsVOIP = 127,
    NotSupported = 128,
    FamilySizeLimitExceeded = 129,
    OfflineAppCacheInvalid = 130,
}

// ─── EMsg as a newtype (aliased values) ──────────────────────────────

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct EMsg(pub u32);

impl EMsg {
    pub const Invalid: Self = Self(0);
    pub const Multi: Self = Self(1);
    pub const ProtobufWrapped: Self = Self(2);
    pub const BaseGeneral: Self = Self(100);
    pub const GenericReply: Self = Self(100);
    pub const DestJobFailed: Self = Self(113);
    pub const Alert: Self = Self(115);
    pub const SCIDRequest: Self = Self(120);
    pub const SCIDResponse: Self = Self(121);
    pub const JobHeartbeat: Self = Self(123);
    pub const HubConnect: Self = Self(124);
    pub const Subscribe: Self = Self(126);
    pub const RouteMessage: Self = Self(127);
    pub const WGRequest: Self = Self(130);
    pub const WGResponse: Self = Self(131);
    pub const KeepAlive: Self = Self(132);
    pub const WebAPIJobRequest: Self = Self(133);
    pub const WebAPIJobResponse: Self = Self(134);
    pub const ClientSessionStart: Self = Self(135);
    pub const ClientSessionEnd: Self = Self(136);
    pub const ClientSessionUpdate: Self = Self(137);
    pub const StatsDeprecated: Self = Self(138);
    pub const Ping: Self = Self(139);
    pub const PingResponse: Self = Self(140);
    pub const Stats: Self = Self(141);
    pub const RequestFullStatsBlock: Self = Self(142);
    pub const LoadDBOCacheItem: Self = Self(143);
    pub const LoadDBOCacheItemResponse: Self = Self(144);
    pub const InvalidateDBOCacheItems: Self = Self(145);
    pub const ServiceMethod: Self = Self(146);
    pub const ServiceMethodResponse: Self = Self(147);
    pub const ClientPackageVersions: Self = Self(148);
    pub const TimestampRequest: Self = Self(149);
    pub const TimestampResponse: Self = Self(150);
    pub const ServiceMethodCallFromClient: Self = Self(151);
    pub const ServiceMethodSendToClient: Self = Self(152);
    pub const BaseShell: Self = Self(200);
    pub const AssignSysID: Self = Self(200);
    pub const Exit: Self = Self(201);
    pub const DirRequest: Self = Self(202);
    pub const DirResponse: Self = Self(203);
    pub const ZipRequest: Self = Self(204);
    pub const ZipResponse: Self = Self(205);
    pub const UpdateRecordResponse: Self = Self(215);
    pub const UpdateCreditCardRequest: Self = Self(221);
    pub const UpdateUserBanResponse: Self = Self(225);
    pub const PrepareToExit: Self = Self(226);
    pub const ContentDescriptionUpdate: Self = Self(227);
    pub const TestResetServer: Self = Self(228);
    pub const UniverseChanged: Self = Self(229);
    pub const ShellConfigInfoUpdate: Self = Self(230);
    pub const RequestWindowsEventLogEntries: Self = Self(233);
    pub const ProvideWindowsEventLogEntries: Self = Self(234);
    pub const ShellSearchLogs: Self = Self(235);
    pub const ShellSearchLogsResponse: Self = Self(236);
    pub const ShellCheckWindowsUpdates: Self = Self(237);
    pub const ShellCheckWindowsUpdatesResponse: Self = Self(238);
    pub const TestFlushDelayedSQL: Self = Self(240);
    pub const TestFlushDelayedSQLResponse: Self = Self(241);
    pub const EnsureExecuteScheduledTask_TEST: Self = Self(242);
    pub const EnsureExecuteScheduledTaskResponse_TEST: Self = Self(243);
    pub const UpdateScheduledTaskEnableState_TEST: Self = Self(244);
    pub const UpdateScheduledTaskEnableStateResponse_TEST: Self = Self(245);
    pub const ContentDescriptionDeltaUpdate: Self = Self(246);
    pub const GMShellAndServerAddressUpdates: Self = Self(247);
    pub const BaseGM: Self = Self(300);
    pub const Heartbeat: Self = Self(300);
    pub const ShellFailed: Self = Self(301);
    pub const ExitShells: Self = Self(307);
    pub const ExitShell: Self = Self(308);
    pub const GracefulExitShell: Self = Self(309);
    pub const LicenseProcessingComplete: Self = Self(316);
    pub const SetTestFlag: Self = Self(317);
    pub const QueuedEmailsComplete: Self = Self(318);
    pub const GMDRMSync: Self = Self(320);
    pub const PhysicalBoxInventory: Self = Self(321);
    pub const UpdateConfigFile: Self = Self(322);
    pub const TestInitDB: Self = Self(323);
    pub const GMWriteConfigToSQL: Self = Self(324);
    pub const GMLoadActivationCodes: Self = Self(325);
    pub const GMQueueForFBS: Self = Self(326);
    pub const GMSchemaConversionResults: Self = Self(327);
    pub const GMWriteShellFailureToSQL: Self = Self(329);
    pub const GMGetServiceMethodRouting: Self = Self(331);
    pub const GMGetServiceMethodRoutingResponse: Self = Self(332);
    pub const GMTestNextBuildSchemaConversion: Self = Self(334);
    pub const GMTestNextBuildSchemaConversionResponse: Self = Self(335);
    pub const ExpectShellRestart: Self = Self(336);
    pub const HotFixProgress: Self = Self(337);
    pub const GMStatsForwardToAdminConnections: Self = Self(338);
    pub const GMGetModifiedConVars: Self = Self(339);
    pub const GMGetModifiedConVarsResponse: Self = Self(340);
    pub const BaseAIS: Self = Self(400);
    pub const AISRequestContentDescription: Self = Self(402);
    pub const AISGetPackageChangeNumber: Self = Self(405);
    pub const AISGetPackageChangeNumberResponse: Self = Self(406);
    pub const AIGetAppGCFlags: Self = Self(423);
    pub const AIGetAppGCFlagsResponse: Self = Self(424);
    pub const AIGetAppList: Self = Self(425);
    pub const AIGetAppListResponse: Self = Self(426);
    pub const AISGetCouponDefinition: Self = Self(429);
    pub const AISGetCouponDefinitionResponse: Self = Self(430);
    pub const AISUpdateSubordinateContentDescription: Self = Self(431);
    pub const AISUpdateSubordinateContentDescriptionResponse: Self = Self(432);
    pub const AISTestEnableGC: Self = Self(433);
    pub const BaseAM: Self = Self(500);
    pub const AMUpdateUserBanRequest: Self = Self(504);
    pub const AMAddLicense: Self = Self(505);
    pub const AMSendSystemIMToUser: Self = Self(508);
    pub const AMExtendLicense: Self = Self(509);
    pub const AMAddMinutesToLicense: Self = Self(510);
    pub const AMCancelLicense: Self = Self(511);
    pub const AMInitPurchase: Self = Self(512);
    pub const AMPurchaseResponse: Self = Self(513);
    pub const AMGetLegacyGameKey: Self = Self(516);
    pub const AMGetLegacyGameKeyResponse: Self = Self(517);
    pub const AMFindHungTransactions: Self = Self(518);
    pub const AMSetAccountTrustedRequest: Self = Self(519);
    pub const AMCancelPurchase: Self = Self(522);
    pub const AMNewChallenge: Self = Self(523);
    pub const AMFixPendingPurchase: Self = Self(525);
    pub const AMFixPendingPurchaseResponse: Self = Self(526);
    pub const AMIsUserBanned: Self = Self(527);
    pub const AMRegisterKey: Self = Self(528);
    pub const AMLoadActivationCodes: Self = Self(529);
    pub const AMLoadActivationCodesResponse: Self = Self(530);
    pub const AMLookupKeyResponse: Self = Self(531);
    pub const AMLookupKey: Self = Self(532);
    pub const AMChatCleanup: Self = Self(533);
    pub const AMClanCleanup: Self = Self(534);
    pub const AMFixPendingRefund: Self = Self(535);
    pub const AMReverseChargeback: Self = Self(536);
    pub const AMReverseChargebackResponse: Self = Self(537);
    pub const AMClanCleanupList: Self = Self(538);
    pub const AMSendCartRepurchase: Self = Self(541);
    pub const AMSendCartRepurchaseResponse: Self = Self(542);
    pub const AllowUserToPlayQuery: Self = Self(550);
    pub const AllowUserToPlayResponse: Self = Self(551);
    pub const AMVerfiyUser: Self = Self(552);
    pub const AMClientNotPlaying: Self = Self(553);
    pub const AMClientRequestFriendship: Self = Self(554);
    pub const AMRelayPublishStatus: Self = Self(555);
    pub const AMInitPurchaseResponse: Self = Self(560);
    pub const AMRevokePurchaseResponse: Self = Self(561);
    pub const AMRefreshGuestPasses: Self = Self(563);
    pub const AMGrantGuestPasses: Self = Self(566);
    pub const AMClanDataUpdated: Self = Self(567);
    pub const AMReloadAccount: Self = Self(568);
    pub const AMClientChatMsgRelay: Self = Self(569);
    pub const AMChatMulti: Self = Self(570);
    pub const AMClientChatInviteRelay: Self = Self(571);
    pub const AMChatInvite: Self = Self(572);
    pub const AMClientJoinChatRelay: Self = Self(573);
    pub const AMClientChatMemberInfoRelay: Self = Self(574);
    pub const AMPublishChatMemberInfo: Self = Self(575);
    pub const AMClientAcceptFriendInvite: Self = Self(576);
    pub const AMChatEnter: Self = Self(577);
    pub const AMClientPublishRemovalFromSource: Self = Self(578);
    pub const AMChatActionResult: Self = Self(579);
    pub const AMFindAccounts: Self = Self(580);
    pub const AMFindAccountsResponse: Self = Self(581);
    pub const AMIsAccountNameInUse: Self = Self(582);
    pub const AMIsAccountNameInUseResponse: Self = Self(583);
    pub const AMSetAccountFlags: Self = Self(584);
    pub const AMCreateClan: Self = Self(586);
    pub const AMCreateClanResponse: Self = Self(587);
    pub const AMGetClanDetails: Self = Self(588);
    pub const AMGetClanDetailsResponse: Self = Self(589);
    pub const AMSetPersonaName: Self = Self(590);
    pub const AMSetAvatar: Self = Self(591);
    pub const AMAuthenticateUser: Self = Self(592);
    pub const AMAuthenticateUserResponse: Self = Self(593);
    pub const AMP2PIntroducerMessage: Self = Self(596);
    pub const ClientChatAction: Self = Self(597);
    pub const AMClientChatActionRelay: Self = Self(598);
    pub const BaseVS: Self = Self(600);
    pub const ReqChallenge: Self = Self(600);
    pub const VACResponse: Self = Self(601);
    pub const ReqChallengeTest: Self = Self(602);
    pub const VSMarkCheat: Self = Self(604);
    pub const VSAddCheat: Self = Self(605);
    pub const VSPurgeCodeModDB: Self = Self(606);
    pub const VSGetChallengeResults: Self = Self(607);
    pub const VSChallengeResultText: Self = Self(608);
    pub const VSReportLingerer: Self = Self(609);
    pub const VSRequestManagedChallenge: Self = Self(610);
    pub const VSLoadDBFinished: Self = Self(611);
    pub const BaseDRMS: Self = Self(625);
    pub const DRMBuildBlobRequest: Self = Self(628);
    pub const DRMBuildBlobResponse: Self = Self(629);
    pub const DRMResolveGuidRequest: Self = Self(630);
    pub const DRMResolveGuidResponse: Self = Self(631);
    pub const DRMVariabilityReport: Self = Self(633);
    pub const DRMVariabilityReportResponse: Self = Self(634);
    pub const DRMStabilityReport: Self = Self(635);
    pub const DRMStabilityReportResponse: Self = Self(636);
    pub const DRMDetailsReportRequest: Self = Self(637);
    pub const DRMDetailsReportResponse: Self = Self(638);
    pub const DRMProcessFile: Self = Self(639);
    pub const DRMAdminUpdate: Self = Self(640);
    pub const DRMAdminUpdateResponse: Self = Self(641);
    pub const DRMSync: Self = Self(642);
    pub const DRMSyncResponse: Self = Self(643);
    pub const DRMProcessFileResponse: Self = Self(644);
    pub const DRMEmptyGuidCache: Self = Self(645);
    pub const DRMEmptyGuidCacheResponse: Self = Self(646);
    pub const BaseCS: Self = Self(650);
    pub const BaseClient: Self = Self(700);
    pub const ClientLogOn_Deprecated: Self = Self(701);
    pub const ClientAnonLogOn_Deprecated: Self = Self(702);
    pub const ClientHeartBeat: Self = Self(703);
    pub const ClientVACResponse: Self = Self(704);
    pub const ClientGamesPlayed_obsolete: Self = Self(705);
    pub const ClientLogOff: Self = Self(706);
    pub const ClientNoUDPConnectivity: Self = Self(707);
    pub const ClientConnectionStats: Self = Self(710);
    pub const ClientPingResponse: Self = Self(712);
    pub const ClientRemoveFriend: Self = Self(714);
    pub const ClientGamesPlayedNoDataBlob: Self = Self(715);
    pub const ClientChangeStatus: Self = Self(716);
    pub const ClientVacStatusResponse: Self = Self(717);
    pub const ClientFriendMsg: Self = Self(718);
    pub const ClientGameConnect_obsolete: Self = Self(719);
    pub const ClientGamesPlayed2_obsolete: Self = Self(720);
    pub const ClientGameEnded_obsolete: Self = Self(721);
    pub const ClientSystemIM: Self = Self(726);
    pub const ClientSystemIMAck: Self = Self(727);
    pub const ClientGetLicenses: Self = Self(728);
    pub const ClientGetLegacyGameKey: Self = Self(730);
    pub const ClientContentServerLogOn_Deprecated: Self = Self(731);
    pub const ClientAckVACBan2: Self = Self(732);
    pub const ClientGetPurchaseReceipts: Self = Self(736);
    pub const ClientGamesPlayed3_obsolete: Self = Self(738);
    pub const ClientAckGuestPass: Self = Self(740);
    pub const ClientRedeemGuestPass: Self = Self(741);
    pub const ClientGamesPlayed: Self = Self(742);
    pub const ClientRegisterKey: Self = Self(743);
    pub const ClientInviteUserToClan: Self = Self(744);
    pub const ClientAcknowledgeClanInvite: Self = Self(745);
    pub const ClientPurchaseWithMachineID: Self = Self(746);
    pub const ClientAppUsageEvent: Self = Self(747);
    pub const ClientLogOnResponse: Self = Self(751);
    pub const ClientSetHeartbeatRate: Self = Self(755);
    pub const ClientNotLoggedOnDeprecated: Self = Self(756);
    pub const ClientLoggedOff: Self = Self(757);
    pub const GSApprove: Self = Self(758);
    pub const GSDeny: Self = Self(759);
    pub const GSKick: Self = Self(760);
    pub const ClientPurchaseResponse: Self = Self(763);
    pub const ClientPing: Self = Self(764);
    pub const ClientNOP: Self = Self(765);
    pub const ClientPersonaState: Self = Self(766);
    pub const ClientFriendsList: Self = Self(767);
    pub const ClientAccountInfo: Self = Self(768);
    pub const ClientNewsUpdate: Self = Self(771);
    pub const ClientGameConnectDeny: Self = Self(773);
    pub const GSStatusReply: Self = Self(774);
    pub const ClientGameConnectTokens: Self = Self(779);
    pub const ClientLicenseList: Self = Self(780);
    pub const ClientVACBanStatus: Self = Self(782);
    pub const ClientEncryptPct: Self = Self(784);
    pub const ClientGetLegacyGameKeyResponse: Self = Self(785);
    pub const ClientAddFriend: Self = Self(791);
    pub const ClientAddFriendResponse: Self = Self(792);
    pub const ClientAckGuestPassResponse: Self = Self(796);
    pub const ClientRedeemGuestPassResponse: Self = Self(797);
    pub const ClientUpdateGuestPassesList: Self = Self(798);
    pub const ClientChatMsg: Self = Self(799);
    pub const ClientChatInvite: Self = Self(800);
    pub const ClientJoinChat: Self = Self(801);
    pub const ClientChatMemberInfo: Self = Self(802);
    pub const ClientLogOnWithCredentials_Deprecated: Self = Self(803);
    pub const ClientPasswordChangeResponse: Self = Self(805);
    pub const ClientChatEnter: Self = Self(807);
    pub const ClientFriendRemovedFromSource: Self = Self(808);
    pub const ClientCreateChat: Self = Self(809);
    pub const ClientCreateChatResponse: Self = Self(810);
    pub const ClientP2PIntroducerMessage: Self = Self(813);
    pub const ClientChatActionResult: Self = Self(814);
    pub const ClientRequestFriendData: Self = Self(815);
    pub const ClientGetUserStats: Self = Self(818);
    pub const ClientGetUserStatsResponse: Self = Self(819);
    pub const ClientStoreUserStats: Self = Self(820);
    pub const ClientStoreUserStatsResponse: Self = Self(821);
    pub const ClientClanState: Self = Self(822);
    pub const ClientServiceModule: Self = Self(830);
    pub const ClientServiceCall: Self = Self(831);
    pub const ClientServiceCallResponse: Self = Self(832);
    pub const ClientNatTraversalStatEvent: Self = Self(839);
    pub const ClientSteamUsageEvent: Self = Self(842);
    pub const ClientCheckPassword: Self = Self(845);
    pub const ClientResetPassword: Self = Self(846);
    pub const ClientCheckPasswordResponse: Self = Self(848);
    pub const ClientResetPasswordResponse: Self = Self(849);
    pub const ClientSessionToken: Self = Self(850);
    pub const ClientDRMProblemReport: Self = Self(851);
    pub const ClientSetIgnoreFriend: Self = Self(855);
    pub const ClientSetIgnoreFriendResponse: Self = Self(856);
    pub const ClientGetAppOwnershipTicket: Self = Self(857);
    pub const ClientGetAppOwnershipTicketResponse: Self = Self(858);
    pub const ClientGetLobbyListResponse: Self = Self(860);
    pub const ClientServerList: Self = Self(880);
    pub const ClientDRMBlobRequest: Self = Self(896);
    pub const ClientDRMBlobResponse: Self = Self(897);
    pub const BaseGameServer: Self = Self(900);
    pub const GSDisconnectNotice: Self = Self(901);
    pub const GSStatus: Self = Self(903);
    pub const GSUserPlaying: Self = Self(905);
    pub const GSStatus2: Self = Self(906);
    pub const GSStatusUpdate_Unused: Self = Self(907);
    pub const GSServerType: Self = Self(908);
    pub const GSPlayerList: Self = Self(909);
    pub const GSGetUserAchievementStatus: Self = Self(910);
    pub const GSGetUserAchievementStatusResponse: Self = Self(911);
    pub const GSGetPlayStats: Self = Self(918);
    pub const GSGetPlayStatsResponse: Self = Self(919);
    pub const GSGetUserGroupStatus: Self = Self(920);
    pub const AMGetUserGroupStatus: Self = Self(921);
    pub const AMGetUserGroupStatusResponse: Self = Self(922);
    pub const GSGetUserGroupStatusResponse: Self = Self(923);
    pub const GSGetReputation: Self = Self(936);
    pub const GSGetReputationResponse: Self = Self(937);
    pub const GSAssociateWithClan: Self = Self(938);
    pub const GSAssociateWithClanResponse: Self = Self(939);
    pub const GSComputeNewPlayerCompatibility: Self = Self(940);
    pub const GSComputeNewPlayerCompatibilityResponse: Self = Self(941);
    pub const BaseAdmin: Self = Self(1000);
    pub const AdminCmd: Self = Self(1000);
    pub const AdminCmdResponse: Self = Self(1004);
    pub const AdminLogListenRequest: Self = Self(1005);
    pub const AdminLogEvent: Self = Self(1006);
    pub const UniverseData: Self = Self(1010);
    pub const AdminSpew: Self = Self(1019);
    pub const AdminConsoleTitle: Self = Self(1020);
    pub const AdminGCSpew: Self = Self(1023);
    pub const AdminGCCommand: Self = Self(1024);
    pub const AdminGCGetCommandList: Self = Self(1025);
    pub const AdminGCGetCommandListResponse: Self = Self(1026);
    pub const FBSConnectionData: Self = Self(1027);
    pub const AdminMsgSpew: Self = Self(1028);
    pub const BaseFBS: Self = Self(1100);
    pub const FBSReqVersion: Self = Self(1100);
    pub const FBSVersionInfo: Self = Self(1101);
    pub const FBSForceRefresh: Self = Self(1102);
    pub const FBSForceBounce: Self = Self(1103);
    pub const FBSDeployPackage: Self = Self(1104);
    pub const FBSDeployResponse: Self = Self(1105);
    pub const FBSUpdateBootstrapper: Self = Self(1106);
    pub const FBSSetState: Self = Self(1107);
    pub const FBSApplyOSUpdates: Self = Self(1108);
    pub const FBSRunCMDScript: Self = Self(1109);
    pub const FBSRebootBox: Self = Self(1110);
    pub const FBSSetBigBrotherMode: Self = Self(1111);
    pub const FBSMinidumpServer: Self = Self(1112);
    pub const FBSDeployHotFixPackage: Self = Self(1114);
    pub const FBSDeployHotFixResponse: Self = Self(1115);
    pub const FBSDownloadHotFix: Self = Self(1116);
    pub const FBSDownloadHotFixResponse: Self = Self(1117);
    pub const FBSUpdateTargetConfigFile: Self = Self(1118);
    pub const FBSApplyAccountCred: Self = Self(1119);
    pub const FBSApplyAccountCredResponse: Self = Self(1120);
    pub const FBSSetShellCount: Self = Self(1121);
    pub const FBSTerminateShell: Self = Self(1122);
    pub const FBSQueryGMForRequest: Self = Self(1123);
    pub const FBSQueryGMResponse: Self = Self(1124);
    pub const FBSTerminateZombies: Self = Self(1125);
    pub const FBSInfoFromBootstrapper: Self = Self(1126);
    pub const FBSRebootBoxResponse: Self = Self(1127);
    pub const FBSBootstrapperPackageRequest: Self = Self(1128);
    pub const FBSBootstrapperPackageResponse: Self = Self(1129);
    pub const FBSBootstrapperGetPackageChunk: Self = Self(1130);
    pub const FBSBootstrapperGetPackageChunkResponse: Self = Self(1131);
    pub const FBSBootstrapperPackageTransferProgress: Self = Self(1132);
    pub const FBSRestartBootstrapper: Self = Self(1133);
    pub const FBSPauseFrozenDumps: Self = Self(1134);
    pub const BaseFileXfer: Self = Self(1200);
    pub const FileXferRequest: Self = Self(1200);
    pub const FileXferResponse: Self = Self(1201);
    pub const FileXferData: Self = Self(1202);
    pub const FileXferEnd: Self = Self(1203);
    pub const FileXferDataAck: Self = Self(1204);
    pub const BaseChannelAuth: Self = Self(1300);
    pub const ChannelAuthChallenge: Self = Self(1300);
    pub const ChannelAuthResponse: Self = Self(1301);
    pub const ChannelAuthResult: Self = Self(1302);
    pub const ChannelEncryptRequest: Self = Self(1303);
    pub const ChannelEncryptResponse: Self = Self(1304);
    pub const ChannelEncryptResult: Self = Self(1305);
    pub const BaseBS: Self = Self(1400);
    pub const BSPurchaseStart: Self = Self(1401);
    pub const BSPurchaseResponse: Self = Self(1402);
    pub const BSAuthenticateCCTrans: Self = Self(1403);
    pub const BSAuthenticateCCTransResponse: Self = Self(1404);
    pub const BSSettleComplete: Self = Self(1406);
    pub const BSInitPayPalTxn: Self = Self(1408);
    pub const BSInitPayPalTxnResponse: Self = Self(1409);
    pub const BSGetPayPalUserInfo: Self = Self(1410);
    pub const BSGetPayPalUserInfoResponse: Self = Self(1411);
    pub const BSPaymentInstrBan: Self = Self(1417);
    pub const BSPaymentInstrBanResponse: Self = Self(1418);
    pub const BSInitGCBankXferTxn: Self = Self(1421);
    pub const BSInitGCBankXferTxnResponse: Self = Self(1422);
    pub const BSCommitGCTxn: Self = Self(1425);
    pub const BSQueryTransactionStatus: Self = Self(1426);
    pub const BSQueryTransactionStatusResponse: Self = Self(1427);
    pub const BSQueryTxnExtendedInfo: Self = Self(1433);
    pub const BSQueryTxnExtendedInfoResponse: Self = Self(1434);
    pub const BSUpdateConversionRates: Self = Self(1435);
    pub const BSPurchaseRunFraudChecks: Self = Self(1437);
    pub const BSPurchaseRunFraudChecksResponse: Self = Self(1438);
    pub const BSQueryBankInformation: Self = Self(1440);
    pub const BSQueryBankInformationResponse: Self = Self(1441);
    pub const BSValidateXsollaSignature: Self = Self(1445);
    pub const BSValidateXsollaSignatureResponse: Self = Self(1446);
    pub const BSQiwiWalletInvoice: Self = Self(1448);
    pub const BSQiwiWalletInvoiceResponse: Self = Self(1449);
    pub const BSUpdateInventoryFromProPack: Self = Self(1450);
    pub const BSUpdateInventoryFromProPackResponse: Self = Self(1451);
    pub const BSSendShippingRequest: Self = Self(1452);
    pub const BSSendShippingRequestResponse: Self = Self(1453);
    pub const BSGetProPackOrderStatus: Self = Self(1454);
    pub const BSGetProPackOrderStatusResponse: Self = Self(1455);
    pub const BSCheckJobRunning: Self = Self(1456);
    pub const BSCheckJobRunningResponse: Self = Self(1457);
    pub const BSResetPackagePurchaseRateLimit: Self = Self(1458);
    pub const BSResetPackagePurchaseRateLimitResponse: Self = Self(1459);
    pub const BSUpdatePaymentData: Self = Self(1460);
    pub const BSUpdatePaymentDataResponse: Self = Self(1461);
    pub const BSGetBillingAddress: Self = Self(1462);
    pub const BSGetBillingAddressResponse: Self = Self(1463);
    pub const BSGetCreditCardInfo: Self = Self(1464);
    pub const BSGetCreditCardInfoResponse: Self = Self(1465);
    pub const BSRemoveExpiredPaymentData: Self = Self(1468);
    pub const BSRemoveExpiredPaymentDataResponse: Self = Self(1469);
    pub const BSConvertToCurrentKeys: Self = Self(1470);
    pub const BSConvertToCurrentKeysResponse: Self = Self(1471);
    pub const BSInitPurchase: Self = Self(1472);
    pub const BSInitPurchaseResponse: Self = Self(1473);
    pub const BSCompletePurchase: Self = Self(1474);
    pub const BSCompletePurchaseResponse: Self = Self(1475);
    pub const BSPruneCardUsageStats: Self = Self(1476);
    pub const BSPruneCardUsageStatsResponse: Self = Self(1477);
    pub const BSStoreBankInformation: Self = Self(1478);
    pub const BSStoreBankInformationResponse: Self = Self(1479);
    pub const BSVerifyPOSAKey: Self = Self(1480);
    pub const BSVerifyPOSAKeyResponse: Self = Self(1481);
    pub const BSReverseRedeemPOSAKey: Self = Self(1482);
    pub const BSReverseRedeemPOSAKeyResponse: Self = Self(1483);
    pub const BSQueryFindCreditCard: Self = Self(1484);
    pub const BSQueryFindCreditCardResponse: Self = Self(1485);
    pub const BSStatusInquiryPOSAKey: Self = Self(1486);
    pub const BSStatusInquiryPOSAKeyResponse: Self = Self(1487);
    pub const BSBoaCompraConfirmProductDelivery: Self = Self(1494);
    pub const BSBoaCompraConfirmProductDeliveryResponse: Self = Self(1495);
    pub const BSGenerateBoaCompraMD5: Self = Self(1496);
    pub const BSGenerateBoaCompraMD5Response: Self = Self(1497);
    pub const BSCommitWPTxn: Self = Self(1498);
    pub const BSCommitAdyenTxn: Self = Self(1499);
    pub const BaseATS: Self = Self(1500);
    pub const ATSStartStressTest: Self = Self(1501);
    pub const ATSStopStressTest: Self = Self(1502);
    pub const ATSRunFailServerTest: Self = Self(1503);
    pub const ATSUFSPerfTestTask: Self = Self(1504);
    pub const ATSUFSPerfTestResponse: Self = Self(1505);
    pub const ATSCycleTCM: Self = Self(1506);
    pub const ATSInitDRMSStressTest: Self = Self(1507);
    pub const ATSCallTest: Self = Self(1508);
    pub const ATSCallTestReply: Self = Self(1509);
    pub const ATSStartExternalStress: Self = Self(1510);
    pub const ATSExternalStressJobStart: Self = Self(1511);
    pub const ATSExternalStressJobQueued: Self = Self(1512);
    pub const ATSExternalStressJobRunning: Self = Self(1513);
    pub const ATSExternalStressJobStopped: Self = Self(1514);
    pub const ATSExternalStressJobStopAll: Self = Self(1515);
    pub const ATSExternalStressActionResult: Self = Self(1516);
    pub const ATSStarted: Self = Self(1517);
    pub const ATSCSPerfTestTask: Self = Self(1518);
    pub const ATSCSPerfTestResponse: Self = Self(1519);
    pub const BaseDP: Self = Self(1600);
    pub const DPSetPublishingState: Self = Self(1601);
    pub const DPUniquePlayersStat: Self = Self(1603);
    pub const DPCloudStats: Self = Self(1612);
    pub const DPGetPlayerCount: Self = Self(1615);
    pub const DPGetPlayerCountResponse: Self = Self(1616);
    pub const DPGameServersPlayersStats: Self = Self(1617);
    pub const ClientDPCheckSpecialSurvey: Self = Self(1620);
    pub const ClientDPCheckSpecialSurveyResponse: Self = Self(1621);
    pub const ClientDPSendSpecialSurveyResponse: Self = Self(1622);
    pub const ClientDPSendSpecialSurveyResponseReply: Self = Self(1623);
    pub const DPStoreSaleStatistics: Self = Self(1624);
    pub const DPPartnerMicroTxns: Self = Self(1628);
    pub const DPPartnerMicroTxnsResponse: Self = Self(1629);
    pub const DPVRUniquePlayersStat: Self = Self(1631);
    pub const BaseCM: Self = Self(1700);
    pub const CMSetAllowState: Self = Self(1701);
    pub const CMSpewAllowState: Self = Self(1702);
    pub const CMSessionRejected: Self = Self(1703);
    pub const CMSetSecrets: Self = Self(1704);
    pub const CMGetSecrets: Self = Self(1705);
    pub const CMRemotePlayReplyPacket: Self = Self(1706);
    pub const BaseGC: Self = Self(2200);
    pub const GCCmdRevive: Self = Self(2203);
    pub const GCCmdDown: Self = Self(2206);
    pub const GCCmdDeploy: Self = Self(2207);
    pub const GCCmdDeployResponse: Self = Self(2208);
    pub const GCCmdSwitch: Self = Self(2209);
    pub const AMRefreshSessions: Self = Self(2210);
    pub const GCAchievementAwarded: Self = Self(2212);
    pub const GCCmdStatus: Self = Self(2216);
    pub const GCRegisterWebInterfaces_Deprecated: Self = Self(2217);
    pub const GCGetAccountDetails_DEPRECATED: Self = Self(2218);
    pub const GCInterAppMessage: Self = Self(2219);
    pub const GCGetEmailTemplate: Self = Self(2220);
    pub const GCGetEmailTemplateResponse: Self = Self(2221);
    pub const GCHRelay: Self = Self(2222);
    pub const GCHRelayToClient: Self = Self(2223);
    pub const GCHUpdateSession: Self = Self(2224);
    pub const GCHRequestUpdateSession: Self = Self(2225);
    pub const GCHRequestStatus: Self = Self(2226);
    pub const GCHRequestStatusResponse: Self = Self(2227);
    pub const GCHAccountVacStatusChange: Self = Self(2228);
    pub const GCHSpawnGC: Self = Self(2229);
    pub const GCHSpawnGCResponse: Self = Self(2230);
    pub const GCHKillGC: Self = Self(2231);
    pub const GCHKillGCResponse: Self = Self(2232);
    pub const GCHAccountTradeBanStatusChange: Self = Self(2233);
    pub const GCHAccountLockStatusChange: Self = Self(2234);
    pub const GCHVacVerificationChange: Self = Self(2235);
    pub const GCHAccountPhoneNumberChange: Self = Self(2236);
    pub const GCHAccountTwoFactorChange: Self = Self(2237);
    pub const GCHInviteUserToLobby: Self = Self(2238);
    pub const GCHUpdateMultipleSessions: Self = Self(2239);
    pub const GCHMarkAppSessionsAuthoritative: Self = Self(2240);
    pub const GCHRecurringSubscriptionStatusChange: Self = Self(2241);
    pub const GCHAppCheersReceived: Self = Self(2242);
    pub const GCHAppCheersGetAllowedTypes: Self = Self(2243);
    pub const GCHAppCheersGetAllowedTypesResponse: Self = Self(2244);
    pub const GCHRoutingRulesFromGCHtoGM: Self = Self(2245);
    pub const GCHRoutingRulesToGCHfromGM: Self = Self(2246);
    pub const UpdateCMMessageRateRules: Self = Self(2247);
    pub const BaseP2P: Self = Self(2500);
    pub const P2PIntroducerMessage: Self = Self(2502);
    pub const BaseSM: Self = Self(2900);
    pub const SMExpensiveReport: Self = Self(2902);
    pub const SMHourlyReport: Self = Self(2903);
    pub const SMPartitionRenames: Self = Self(2905);
    pub const SMMonitorSpace: Self = Self(2906);
    pub const SMTestNextBuildSchemaConversion: Self = Self(2907);
    pub const SMTestNextBuildSchemaConversionResponse: Self = Self(2908);
    pub const BaseTest: Self = Self(3000);
    pub const FailServer: Self = Self(3000);
    pub const JobHeartbeatTest: Self = Self(3001);
    pub const JobHeartbeatTestResponse: Self = Self(3002);
    pub const BaseFTSRange: Self = Self(3100);
    pub const BaseCCSRange: Self = Self(3150);
    pub const CCSDeleteAllCommentsByAuthor: Self = Self(3161);
    pub const CCSDeleteAllCommentsByAuthorResponse: Self = Self(3162);
    pub const BaseLBSRange: Self = Self(3200);
    pub const LBSSetScore: Self = Self(3201);
    pub const LBSSetScoreResponse: Self = Self(3202);
    pub const LBSFindOrCreateLB: Self = Self(3203);
    pub const LBSFindOrCreateLBResponse: Self = Self(3204);
    pub const LBSGetLBEntries: Self = Self(3205);
    pub const LBSGetLBEntriesResponse: Self = Self(3206);
    pub const LBSGetLBList: Self = Self(3207);
    pub const LBSGetLBListResponse: Self = Self(3208);
    pub const LBSSetLBDetails: Self = Self(3209);
    pub const LBSDeleteLB: Self = Self(3210);
    pub const LBSDeleteLBEntry: Self = Self(3211);
    pub const LBSResetLB: Self = Self(3212);
    pub const LBSResetLBResponse: Self = Self(3213);
    pub const LBSDeleteLBResponse: Self = Self(3214);
    pub const BaseOGS: Self = Self(3400);
    pub const OGSBeginSession: Self = Self(3401);
    pub const OGSBeginSessionResponse: Self = Self(3402);
    pub const OGSEndSession: Self = Self(3403);
    pub const OGSEndSessionResponse: Self = Self(3404);
    pub const OGSWriteAppSessionRow: Self = Self(3406);
    pub const BaseBRP: Self = Self(3600);
    pub const BaseAMRange2: Self = Self(4000);
    pub const AMCreateChat: Self = Self(4001);
    pub const AMCreateChatResponse: Self = Self(4002);
    pub const AMGetAccountEmailAddress: Self = Self(4006);
    pub const AMGetAccountEmailAddressResponse: Self = Self(4007);
    pub const AMRequestClanData: Self = Self(4008);
    pub const AMRouteToClients: Self = Self(4009);
    pub const AMLeaveClan: Self = Self(4010);
    pub const AMClanPermissions: Self = Self(4011);
    pub const AMClanPermissionsResponse: Self = Self(4012);
    pub const AMCreateClanEventDummyForRateLimiting: Self = Self(4013);
    pub const AMUpdateClanEventDummyForRateLimiting: Self = Self(4015);
    pub const AMSetClanPermissionSettings: Self = Self(4021);
    pub const AMSetClanPermissionSettingsResponse: Self = Self(4022);
    pub const AMGetClanPermissionSettings: Self = Self(4023);
    pub const AMGetClanPermissionSettingsResponse: Self = Self(4024);
    pub const AMPublishChatRoomInfo: Self = Self(4025);
    pub const ClientChatRoomInfo: Self = Self(4026);
    pub const AMGetClanHistory: Self = Self(4039);
    pub const AMGetClanHistoryResponse: Self = Self(4040);
    pub const AMGetClanPermissionBits: Self = Self(4041);
    pub const AMGetClanPermissionBitsResponse: Self = Self(4042);
    pub const AMSetClanPermissionBits: Self = Self(4043);
    pub const AMSetClanPermissionBitsResponse: Self = Self(4044);
    pub const AMSessionInfoRequest: Self = Self(4045);
    pub const AMSessionInfoResponse: Self = Self(4046);
    pub const AMValidateWGToken: Self = Self(4047);
    pub const AMGetClanRank: Self = Self(4050);
    pub const AMGetClanRankResponse: Self = Self(4051);
    pub const AMSetClanRank: Self = Self(4052);
    pub const AMSetClanRankResponse: Self = Self(4053);
    pub const AMGetClanPOTW: Self = Self(4054);
    pub const AMGetClanPOTWResponse: Self = Self(4055);
    pub const AMSetClanPOTW: Self = Self(4056);
    pub const AMSetClanPOTWResponse: Self = Self(4057);
    pub const AMDumpUser: Self = Self(4059);
    pub const AMKickUserFromClan: Self = Self(4060);
    pub const AMAddFounderToClan: Self = Self(4061);
    pub const AMValidateWGTokenResponse: Self = Self(4062);
    pub const AMSetAccountDetails: Self = Self(4064);
    pub const AMGetChatBanList: Self = Self(4065);
    pub const AMGetChatBanListResponse: Self = Self(4066);
    pub const AMUnBanFromChat: Self = Self(4067);
    pub const AMSetClanDetails: Self = Self(4068);
    pub const UGSGetUserGameStats: Self = Self(4073);
    pub const UGSGetUserGameStatsResponse: Self = Self(4074);
    pub const AMCheckClanMembership: Self = Self(4075);
    pub const AMGetClanMembers: Self = Self(4076);
    pub const AMGetClanMembersResponse: Self = Self(4077);
    pub const AMNotifyChatOfClanChange: Self = Self(4079);
    pub const AMResubmitPurchase: Self = Self(4080);
    pub const AMAddFriend: Self = Self(4081);
    pub const AMAddFriendResponse: Self = Self(4082);
    pub const AMRemoveFriend: Self = Self(4083);
    pub const AMDumpClan: Self = Self(4084);
    pub const AMChangeClanOwner: Self = Self(4085);
    pub const AMCancelEasyCollect: Self = Self(4086);
    pub const AMCancelEasyCollectResponse: Self = Self(4087);
    pub const AMClansInCommon: Self = Self(4090);
    pub const AMClansInCommonResponse: Self = Self(4091);
    pub const AMIsValidAccountID: Self = Self(4092);
    pub const AMWipeFriendsList: Self = Self(4095);
    pub const AMSetIgnored: Self = Self(4096);
    pub const AMClansInCommonCountResponse: Self = Self(4097);
    pub const AMFriendsList: Self = Self(4098);
    pub const AMFriendsListResponse: Self = Self(4099);
    pub const AMFriendsInCommon: Self = Self(4100);
    pub const AMFriendsInCommonResponse: Self = Self(4101);
    pub const AMFriendsInCommonCountResponse: Self = Self(4102);
    pub const AMClansInCommonCount: Self = Self(4103);
    pub const AMChallengeVerdict: Self = Self(4104);
    pub const AMChallengeNotification: Self = Self(4105);
    pub const AMFindGSByIP: Self = Self(4106);
    pub const AMFoundGSByIP: Self = Self(4107);
    pub const AMGiftRevoked: Self = Self(4108);
    pub const AMUserClanList: Self = Self(4110);
    pub const AMUserClanListResponse: Self = Self(4111);
    pub const AMGetAccountDetails2: Self = Self(4112);
    pub const AMGetAccountDetailsResponse2: Self = Self(4113);
    pub const AMSetCommunityProfileSettings: Self = Self(4114);
    pub const AMSetCommunityProfileSettingsResponse: Self = Self(4115);
    pub const AMGetCommunityPrivacyState: Self = Self(4116);
    pub const AMGetCommunityPrivacyStateResponse: Self = Self(4117);
    pub const AMCheckClanInviteRateLimiting: Self = Self(4118);
    pub const UGSGetUserAchievementStatus: Self = Self(4119);
    pub const AMGetIgnored: Self = Self(4120);
    pub const AMGetIgnoredResponse: Self = Self(4121);
    pub const AMSetIgnoredResponse: Self = Self(4122);
    pub const AMSetFriendRelationshipNone: Self = Self(4123);
    pub const AMGetFriendRelationship: Self = Self(4124);
    pub const AMGetFriendRelationshipResponse: Self = Self(4125);
    pub const AMServiceModulesCache: Self = Self(4126);
    pub const AMServiceModulesCall: Self = Self(4127);
    pub const AMServiceModulesCallResponse: Self = Self(4128);
    pub const CommunityAddFriendNews: Self = Self(4140);
    pub const AMFindClanUser: Self = Self(4143);
    pub const AMFindClanUserResponse: Self = Self(4144);
    pub const AMBanFromChat: Self = Self(4145);
    pub const AMGetUserNewsSubscriptions: Self = Self(4147);
    pub const AMGetUserNewsSubscriptionsResponse: Self = Self(4148);
    pub const AMSetUserNewsSubscriptions: Self = Self(4149);
    pub const AMSendQueuedEmails: Self = Self(4152);
    pub const AMSetLicenseFlags: Self = Self(4153);
    pub const AMGetAccountStatus: Self = Self(4158);
    pub const AMGetAccountStatusResponse: Self = Self(4159);
    pub const AMEditBanReason: Self = Self(4160);
    pub const AMCheckClanMembershipResponse: Self = Self(4161);
    pub const AMProbeClanMembershipList: Self = Self(4162);
    pub const AMProbeClanMembershipListResponse: Self = Self(4163);
    pub const UGSGetUserAchievementStatusResponse: Self = Self(4164);
    pub const AMGetFriendsLobbies: Self = Self(4165);
    pub const AMGetFriendsLobbiesResponse: Self = Self(4166);
    pub const AMGetUserFriendNewsResponse: Self = Self(4172);
    pub const CommunityGetUserFriendNews: Self = Self(4173);
    pub const AMGetUserClansNewsResponse: Self = Self(4174);
    pub const AMGetUserClansNews: Self = Self(4175);
    pub const AMGetPreviousCBAccount: Self = Self(4184);
    pub const AMGetPreviousCBAccountResponse: Self = Self(4185);
    pub const AMGetUserLicenseHistory: Self = Self(4190);
    pub const AMGetUserLicenseHistoryResponse: Self = Self(4191);
    pub const AMSupportChangePassword: Self = Self(4194);
    pub const AMSupportChangeEmail: Self = Self(4195);
    pub const AMResetUserVerificationGSByIP: Self = Self(4197);
    pub const AMUpdateGSPlayStats: Self = Self(4198);
    pub const AMSupportEnableOrDisable: Self = Self(4199);
    pub const AMGetPurchaseStatus: Self = Self(4206);
    pub const AMSupportIsAccountEnabled: Self = Self(4209);
    pub const AMSupportIsAccountEnabledResponse: Self = Self(4210);
    pub const UGSGetUserStats: Self = Self(4211);
    pub const AMGSSearch: Self = Self(4213);
    pub const ChatServerRouteFriendMsg: Self = Self(4219);
    pub const AMTicketAuthRequestOrResponse: Self = Self(4220);
    pub const AMAddFreeLicense: Self = Self(4224);
    pub const AMValidateEmailLink: Self = Self(4231);
    pub const AMValidateEmailLinkResponse: Self = Self(4232);
    pub const UGSStoreUserStats: Self = Self(4236);
    pub const AMDeleteStoredCard: Self = Self(4241);
    pub const AMRevokeLegacyGameKeys: Self = Self(4242);
    pub const AMGetWalletDetails: Self = Self(4244);
    pub const AMGetWalletDetailsResponse: Self = Self(4245);
    pub const AMDeleteStoredPaymentInfo: Self = Self(4246);
    pub const AMGetStoredPaymentSummary: Self = Self(4247);
    pub const AMGetStoredPaymentSummaryResponse: Self = Self(4248);
    pub const AMGetWalletConversionRate: Self = Self(4249);
    pub const AMGetWalletConversionRateResponse: Self = Self(4250);
    pub const AMConvertWallet: Self = Self(4251);
    pub const AMConvertWalletResponse: Self = Self(4252);
    pub const AMSetPreApproval: Self = Self(4255);
    pub const AMSetPreApprovalResponse: Self = Self(4256);
    pub const AMCreateRefund: Self = Self(4258);
    pub const AMCreateChargeback: Self = Self(4260);
    pub const AMCreateDispute: Self = Self(4262);
    pub const AMClearDispute: Self = Self(4264);
    pub const AMCreateFinancialAdjustment: Self = Self(4265);
    pub const AMPlayerNicknameList: Self = Self(4266);
    pub const AMPlayerNicknameListResponse: Self = Self(4267);
    pub const AMGetUserCurrentGameInfo: Self = Self(4269);
    pub const AMGetUserCurrentGameInfoResponse: Self = Self(4270);
    pub const AMGetGSPlayerList: Self = Self(4271);
    pub const AMGetGSPlayerListResponse: Self = Self(4272);
    pub const AMGetSteamIDForMicroTxn: Self = Self(4278);
    pub const AMGetSteamIDForMicroTxnResponse: Self = Self(4279);
    pub const AMSetPartnerMember: Self = Self(4280);
    pub const AMRemovePublisherUser: Self = Self(4281);
    pub const AMGetUserLicenseList: Self = Self(4282);
    pub const AMGetUserLicenseListResponse: Self = Self(4283);
    pub const AMReloadGameGroupPolicy: Self = Self(4284);
    pub const AMAddFreeLicenseResponse: Self = Self(4285);
    pub const AMVACStatusUpdate: Self = Self(4286);
    pub const AMGetAccountDetails: Self = Self(4287);
    pub const AMGetAccountDetailsResponse: Self = Self(4288);
    pub const AMGetPlayerLinkDetails: Self = Self(4289);
    pub const AMGetPlayerLinkDetailsResponse: Self = Self(4290);
    pub const AMGetAccountFlagsForWGSpoofing: Self = Self(4294);
    pub const AMGetAccountFlagsForWGSpoofingResponse: Self = Self(4295);
    pub const AMGetClanOfficers: Self = Self(4298);
    pub const AMGetClanOfficersResponse: Self = Self(4299);
    pub const AMNameChange: Self = Self(4300);
    pub const AMGetNameHistory: Self = Self(4301);
    pub const AMGetNameHistoryResponse: Self = Self(4302);
    pub const AMSupportRemoveAccountSecurity: Self = Self(4307);
    pub const AMIsAccountInCaptchaGracePeriod: Self = Self(4308);
    pub const AMIsAccountInCaptchaGracePeriodResponse: Self = Self(4309);
    pub const AMAccountPS3Unlink: Self = Self(4310);
    pub const AMAccountPS3UnlinkResponse: Self = Self(4311);
    pub const UGSStoreUserStatsResponse: Self = Self(4312);
    pub const AMGetAccountPSNInfo: Self = Self(4313);
    pub const AMGetAccountPSNInfoResponse: Self = Self(4314);
    pub const AMAuthenticatedPlayerList: Self = Self(4315);
    pub const AMGetUserGifts: Self = Self(4316);
    pub const AMGetUserGiftsResponse: Self = Self(4317);
    pub const AMTransferLockedGifts: Self = Self(4320);
    pub const AMTransferLockedGiftsResponse: Self = Self(4321);
    pub const AMPlayerHostedOnGameServer: Self = Self(4322);
    pub const AMGetAccountBanInfo: Self = Self(4323);
    pub const AMGetAccountBanInfoResponse: Self = Self(4324);
    pub const AMRecordBanEnforcement: Self = Self(4325);
    pub const AMRollbackGiftTransfer: Self = Self(4326);
    pub const AMRollbackGiftTransferResponse: Self = Self(4327);
    pub const AMHandlePendingTransaction: Self = Self(4328);
    pub const AMRequestClanDetails: Self = Self(4329);
    pub const AMDeleteStoredPaypalAgreement: Self = Self(4330);
    pub const AMGameServerUpdate: Self = Self(4331);
    pub const AMGameServerRemove: Self = Self(4332);
    pub const AMGetPaypalAgreements: Self = Self(4333);
    pub const AMGetPaypalAgreementsResponse: Self = Self(4334);
    pub const AMGameServerPlayerCompatibilityCheck: Self = Self(4335);
    pub const AMGameServerPlayerCompatibilityCheckResponse: Self = Self(4336);
    pub const AMRenewLicense: Self = Self(4337);
    pub const AMGetAccountCommunityBanInfo: Self = Self(4338);
    pub const AMGetAccountCommunityBanInfoResponse: Self = Self(4339);
    pub const AMGameServerAccountChangePassword: Self = Self(4340);
    pub const AMGameServerAccountDeleteAccount: Self = Self(4341);
    pub const AMRenewAgreement: Self = Self(4342);
    pub const AMXsollaPayment: Self = Self(4344);
    pub const AMXsollaPaymentResponse: Self = Self(4345);
    pub const AMAcctAllowedToPurchase: Self = Self(4346);
    pub const AMAcctAllowedToPurchaseResponse: Self = Self(4347);
    pub const AMSwapKioskDeposit: Self = Self(4348);
    pub const AMSwapKioskDepositResponse: Self = Self(4349);
    pub const AMSetUserGiftUnowned: Self = Self(4350);
    pub const AMSetUserGiftUnownedResponse: Self = Self(4351);
    pub const AMClaimUnownedUserGift: Self = Self(4352);
    pub const AMClaimUnownedUserGiftResponse: Self = Self(4353);
    pub const AMSetClanName: Self = Self(4354);
    pub const AMSetClanNameResponse: Self = Self(4355);
    pub const AMGrantCoupon: Self = Self(4356);
    pub const AMGrantCouponResponse: Self = Self(4357);
    pub const AMIsPackageRestrictedInUserCountry: Self = Self(4358);
    pub const AMIsPackageRestrictedInUserCountryResponse: Self = Self(4359);
    pub const AMHandlePendingTransactionResponse: Self = Self(4360);
    pub const AMGrantGuestPasses2: Self = Self(4361);
    pub const AMGrantGuestPasses2Response: Self = Self(4362);
    pub const AMGetPlayerBanDetails: Self = Self(4365);
    pub const AMGetPlayerBanDetailsResponse: Self = Self(4366);
    pub const AMFinalizePurchase: Self = Self(4367);
    pub const AMFinalizePurchaseResponse: Self = Self(4368);
    pub const AMPersonaChangeResponse: Self = Self(4372);
    pub const AMGetClanDetailsForForumCreation: Self = Self(4373);
    pub const AMGetClanDetailsForForumCreationResponse: Self = Self(4374);
    pub const AMGetPendingNotificationCount: Self = Self(4375);
    pub const AMGetPendingNotificationCountResponse: Self = Self(4376);
    pub const AMPasswordHashUpgrade: Self = Self(4377);
    pub const AMBoaCompraPayment: Self = Self(4380);
    pub const AMBoaCompraPaymentResponse: Self = Self(4381);
    pub const AMCompleteExternalPurchase: Self = Self(4383);
    pub const AMCompleteExternalPurchaseResponse: Self = Self(4384);
    pub const AMResolveNegativeWalletCredits: Self = Self(4385);
    pub const AMResolveNegativeWalletCreditsResponse: Self = Self(4386);
    pub const AMPlayerGetClanBasicDetails: Self = Self(4389);
    pub const AMPlayerGetClanBasicDetailsResponse: Self = Self(4390);
    pub const AMMOLPayment: Self = Self(4391);
    pub const AMMOLPaymentResponse: Self = Self(4392);
    pub const GetUserIPCountry: Self = Self(4393);
    pub const GetUserIPCountryResponse: Self = Self(4394);
    pub const NotificationOfSuspiciousActivity: Self = Self(4395);
    pub const AMDegicaPayment: Self = Self(4396);
    pub const AMDegicaPaymentResponse: Self = Self(4397);
    pub const AMEClubPayment: Self = Self(4398);
    pub const AMEClubPaymentResponse: Self = Self(4399);
    pub const AMPayPalPaymentsHubPayment: Self = Self(4400);
    pub const AMPayPalPaymentsHubPaymentResponse: Self = Self(4401);
    pub const AMTwoFactorRecoverAuthenticatorRequest: Self = Self(4402);
    pub const AMTwoFactorRecoverAuthenticatorResponse: Self = Self(4403);
    pub const AMSmart2PayPayment: Self = Self(4404);
    pub const AMSmart2PayPaymentResponse: Self = Self(4405);
    pub const AMValidatePasswordResetCodeAndSendSmsRequest: Self = Self(4406);
    pub const AMValidatePasswordResetCodeAndSendSmsResponse: Self = Self(4407);
    pub const AMGetAccountResetDetailsRequest: Self = Self(4408);
    pub const AMGetAccountResetDetailsResponse: Self = Self(4409);
    pub const AMBitPayPayment: Self = Self(4410);
    pub const AMBitPayPaymentResponse: Self = Self(4411);
    pub const AMSendAccountInfoUpdate: Self = Self(4412);
    pub const AMSendScheduledGift: Self = Self(4413);
    pub const AMNodwinPayment: Self = Self(4414);
    pub const AMNodwinPaymentResponse: Self = Self(4415);
    pub const AMResolveWalletRevoke: Self = Self(4416);
    pub const AMResolveWalletReverseRevoke: Self = Self(4417);
    pub const AMFundedPayment: Self = Self(4418);
    pub const AMFundedPaymentResponse: Self = Self(4419);
    pub const AMRequestPersonaUpdateForChatServer: Self = Self(4420);
    pub const AMPerfectWorldPayment: Self = Self(4421);
    pub const AMPerfectWorldPaymentResponse: Self = Self(4422);
    pub const AMECommPayPayment: Self = Self(4423);
    pub const AMECommPayPaymentResponse: Self = Self(4424);
    pub const AMSetRemoteClientID: Self = Self(4425);
    pub const BasePSRange: Self = Self(5000);
    pub const PSIsValidShoppingCart: Self = Self(5003);
    pub const PSIsValidShoppingCartResponse: Self = Self(5004);
    pub const PSGetShoppingCartContents: Self = Self(5009);
    pub const PSGetShoppingCartContentsResponse: Self = Self(5010);
    pub const PSAddWalletCreditToShoppingCart: Self = Self(5011);
    pub const PSAddWalletCreditToShoppingCartResponse: Self = Self(5012);
    pub const PSGetAccountCartContents: Self = Self(5013);
    pub const PSGetAccountCartContentsResponse: Self = Self(5014);
    pub const BaseUFSRange: Self = Self(5200);
    pub const UFSReloadPartitionInfo: Self = Self(5215);
    pub const UFSSynchronizeFile: Self = Self(5217);
    pub const UFSSynchronizeFileResponse: Self = Self(5218);
    pub const ClientUFSGetUGCDetails: Self = Self(5226);
    pub const ClientUFSGetUGCDetailsResponse: Self = Self(5227);
    pub const UFSUpdateFileFlags: Self = Self(5228);
    pub const UFSUpdateFileFlagsResponse: Self = Self(5229);
    pub const ClientUFSGetSingleFileInfo: Self = Self(5230);
    pub const ClientUFSGetSingleFileInfoResponse: Self = Self(5231);
    pub const ClientUFSShareFile: Self = Self(5232);
    pub const ClientUFSShareFileResponse: Self = Self(5233);
    pub const UFSReloadAccount: Self = Self(5234);
    pub const UFSReloadAccountResponse: Self = Self(5235);
    pub const UFSUpdateRecordBatched: Self = Self(5236);
    pub const UFSUpdateRecordBatchedResponse: Self = Self(5237);
    pub const UFSMigrateFile: Self = Self(5238);
    pub const UFSMigrateFileResponse: Self = Self(5239);
    pub const UFSGetUGCURLs: Self = Self(5240);
    pub const UFSGetUGCURLsResponse: Self = Self(5241);
    pub const UFSHttpUploadFileFinishRequest: Self = Self(5242);
    pub const UFSHttpUploadFileFinishResponse: Self = Self(5243);
    pub const UFSDownloadStartRequest: Self = Self(5244);
    pub const UFSDownloadStartResponse: Self = Self(5245);
    pub const UFSDownloadChunkRequest: Self = Self(5246);
    pub const UFSDownloadChunkResponse: Self = Self(5247);
    pub const UFSDownloadFinishRequest: Self = Self(5248);
    pub const UFSDownloadFinishResponse: Self = Self(5249);
    pub const UFSFlushURLCache: Self = Self(5250);
    pub const UFSMigrateFileAppID: Self = Self(5253);
    pub const UFSMigrateFileAppIDResponse: Self = Self(5254);
    pub const BaseClient2: Self = Self(5400);
    pub const ClientRequestForgottenPasswordEmail: Self = Self(5401);
    pub const ClientRequestForgottenPasswordEmailResponse: Self = Self(5402);
    pub const ClientCreateAccountResponse: Self = Self(5403);
    pub const ClientResetForgottenPassword: Self = Self(5404);
    pub const ClientResetForgottenPasswordResponse: Self = Self(5405);
    pub const ClientInformOfResetForgottenPassword: Self = Self(5407);
    pub const ClientInformOfResetForgottenPasswordResponse: Self = Self(5408);
    pub const ClientAnonUserLogOn_Deprecated: Self = Self(5409);
    pub const ClientGamesPlayedWithDataBlob: Self = Self(5410);
    pub const ClientUpdateUserGameInfo: Self = Self(5411);
    pub const ClientFileToDownload: Self = Self(5412);
    pub const ClientFileToDownloadResponse: Self = Self(5413);
    pub const ClientLBSSetScore: Self = Self(5414);
    pub const ClientLBSSetScoreResponse: Self = Self(5415);
    pub const ClientLBSFindOrCreateLB: Self = Self(5416);
    pub const ClientLBSFindOrCreateLBResponse: Self = Self(5417);
    pub const ClientLBSGetLBEntries: Self = Self(5418);
    pub const ClientLBSGetLBEntriesResponse: Self = Self(5419);
    pub const ClientChatDeclined: Self = Self(5426);
    pub const ClientFriendMsgIncoming: Self = Self(5427);
    pub const ClientAuthList_Deprecated: Self = Self(5428);
    pub const ClientTicketAuthComplete: Self = Self(5429);
    pub const ClientIsLimitedAccount: Self = Self(5430);
    pub const ClientRequestAuthList: Self = Self(5431);
    pub const ClientAuthList: Self = Self(5432);
    pub const ClientStat_Deprecated: Self = Self(5433);
    pub const ClientP2PConnectionInfo: Self = Self(5434);
    pub const ClientP2PConnectionFailInfo: Self = Self(5435);
    pub const ClientGetDepotDecryptionKey: Self = Self(5438);
    pub const ClientGetDepotDecryptionKeyResponse: Self = Self(5439);
    pub const ClientEnableTestLicense: Self = Self(5443);
    pub const ClientEnableTestLicenseResponse: Self = Self(5444);
    pub const ClientDisableTestLicense: Self = Self(5445);
    pub const ClientDisableTestLicenseResponse: Self = Self(5446);
    pub const ClientCheckAppBetaPassword: Self = Self(5450);
    pub const ClientCheckAppBetaPasswordResponse: Self = Self(5451);
    pub const ClientToGC: Self = Self(5452);
    pub const ClientFromGC: Self = Self(5453);
    pub const ClientEmailAddrInfo: Self = Self(5456);
    pub const ClientPasswordChange3: Self = Self(5457);
    pub const ClientEmailChange3: Self = Self(5458);
    pub const ClientPersonalQAChange3: Self = Self(5459);
    pub const ClientResetForgottenPassword3: Self = Self(5460);
    pub const ClientRequestForgottenPasswordEmail3: Self = Self(5461);
    pub const ClientNewLoginKey: Self = Self(5463);
    pub const ClientNewLoginKeyAccepted_Deprecated: Self = Self(5464);
    pub const ClientLogOnWithHash_Deprecated: Self = Self(5465);
    pub const ClientStoreUserStats2: Self = Self(5466);
    pub const ClientStatsUpdated: Self = Self(5467);
    pub const ClientRequestedClientStats_Deprecated: Self = Self(5480);
    pub const ClientStat2Int32_Deprecated: Self = Self(5481);
    pub const ClientStat2_Deprecated: Self = Self(5482);
    pub const ClientDRMDownloadRequest: Self = Self(5485);
    pub const ClientDRMDownloadResponse: Self = Self(5486);
    pub const ClientDRMFinalResult: Self = Self(5487);
    pub const ClientGetFriendsWhoPlayGame: Self = Self(5488);
    pub const ClientGetFriendsWhoPlayGameResponse: Self = Self(5489);
    pub const ClientOGSBeginSession: Self = Self(5490);
    pub const ClientOGSBeginSessionResponse: Self = Self(5491);
    pub const ClientOGSEndSession: Self = Self(5492);
    pub const ClientOGSEndSessionResponse: Self = Self(5493);
    pub const ClientOGSWriteRow: Self = Self(5494);
    pub const ClientGetPeerContentInfo: Self = Self(5495);
    pub const ClientGetPeerContentInfoResponse: Self = Self(5496);
    pub const ClientStartPeerContentServer: Self = Self(5497);
    pub const ClientStartPeerContentServerResponse: Self = Self(5498);
    pub const ClientServerUnavailable: Self = Self(5500);
    pub const ClientServersAvailable: Self = Self(5501);
    pub const ClientRegisterAuthTicketWithCM: Self = Self(5502);
    pub const ClientGCMsgFailed: Self = Self(5503);
    pub const ClientMicroTxnAuthRequest: Self = Self(5504);
    pub const ClientMicroTxnAuthorize: Self = Self(5505);
    pub const ClientMicroTxnAuthorizeResponse: Self = Self(5506);
    pub const ClientGetMicroTxnInfo: Self = Self(5508);
    pub const ClientGetMicroTxnInfoResponse: Self = Self(5509);
    pub const ClientDeregisterWithServer: Self = Self(5511);
    pub const ClientSubscribeToPersonaFeed: Self = Self(5512);
    pub const ClientLogon: Self = Self(5514);
    pub const ClientGetClientDetails: Self = Self(5515);
    pub const ClientGetClientDetailsResponse: Self = Self(5516);
    pub const ClientReportOverlayDetourFailure: Self = Self(5517);
    pub const ClientGetClientAppList: Self = Self(5518);
    pub const ClientGetClientAppListResponse: Self = Self(5519);
    pub const ClientInstallClientApp: Self = Self(5520);
    pub const ClientInstallClientAppResponse: Self = Self(5521);
    pub const ClientUninstallClientApp: Self = Self(5522);
    pub const ClientUninstallClientAppResponse: Self = Self(5523);
    pub const ClientSetClientAppUpdateState: Self = Self(5524);
    pub const ClientSetClientAppUpdateStateResponse: Self = Self(5525);
    pub const ClientRequestEncryptedAppTicket: Self = Self(5526);
    pub const ClientRequestEncryptedAppTicketResponse: Self = Self(5527);
    pub const ClientWalletInfoUpdate: Self = Self(5528);
    pub const ClientLBSSetUGC: Self = Self(5529);
    pub const ClientLBSSetUGCResponse: Self = Self(5530);
    pub const ClientAMGetClanOfficers: Self = Self(5531);
    pub const ClientAMGetClanOfficersResponse: Self = Self(5532);
    pub const ClientFriendProfileInfo: Self = Self(5535);
    pub const ClientFriendProfileInfoResponse: Self = Self(5536);
    pub const ClientScreenshotsChanged: Self = Self(5543);
    pub const ClientLaunchClientApp: Self = Self(5544);
    pub const ClientLaunchClientAppResponse: Self = Self(5545);
    pub const ClientRequestAccountData: Self = Self(5549);
    pub const ClientRequestAccountDataResponse: Self = Self(5550);
    pub const ClientResetForgottenPassword4: Self = Self(5551);
    pub const ClientHideFriend: Self = Self(5552);
    pub const ClientFriendsGroupsList: Self = Self(5553);
    pub const ClientGetClanActivityCounts: Self = Self(5554);
    pub const ClientGetClanActivityCountsResponse: Self = Self(5555);
    pub const ClientOGSReportString: Self = Self(5556);
    pub const ClientOGSReportBug: Self = Self(5557);
    pub const ClientSentLogs: Self = Self(5558);
    pub const ClientLogonGameServer: Self = Self(5559);
    pub const AMClientCreateFriendsGroup: Self = Self(5560);
    pub const AMClientCreateFriendsGroupResponse: Self = Self(5561);
    pub const AMClientDeleteFriendsGroup: Self = Self(5562);
    pub const AMClientDeleteFriendsGroupResponse: Self = Self(5563);
    pub const AMClientManageFriendsGroup: Self = Self(5564);
    pub const AMClientManageFriendsGroupResponse: Self = Self(5565);
    pub const AMClientAddFriendToGroup: Self = Self(5566);
    pub const AMClientAddFriendToGroupResponse: Self = Self(5567);
    pub const AMClientRemoveFriendFromGroup: Self = Self(5568);
    pub const AMClientRemoveFriendFromGroupResponse: Self = Self(5569);
    pub const ClientAMGetPersonaNameHistory: Self = Self(5570);
    pub const ClientAMGetPersonaNameHistoryResponse: Self = Self(5571);
    pub const ClientRequestFreeLicense: Self = Self(5572);
    pub const ClientRequestFreeLicenseResponse: Self = Self(5573);
    pub const ClientDRMDownloadRequestWithCrashData: Self = Self(5574);
    pub const ClientAuthListAck: Self = Self(5575);
    pub const ClientItemAnnouncements: Self = Self(5576);
    pub const ClientRequestItemAnnouncements: Self = Self(5577);
    pub const ClientFriendMsgEchoToSender: Self = Self(5578);
    pub const ClientCommentNotifications: Self = Self(5582);
    pub const ClientRequestCommentNotifications: Self = Self(5583);
    pub const ClientPersonaChangeResponse: Self = Self(5584);
    pub const ClientRequestWebAPIAuthenticateUserNonce_Deprecated: Self = Self(5585);
    pub const ClientRequestWebAPIAuthenticateUserNonceResponse_Deprecated: Self = Self(5586);
    pub const ClientPlayerNicknameList: Self = Self(5587);
    pub const AMClientSetPlayerNickname: Self = Self(5588);
    pub const AMClientSetPlayerNicknameResponse: Self = Self(5589);
    pub const ClientGetNumberOfCurrentPlayersDP: Self = Self(5592);
    pub const ClientGetNumberOfCurrentPlayersDPResponse: Self = Self(5593);
    pub const ClientServiceMethodLegacy: Self = Self(5594);
    pub const ClientServiceMethodLegacyResponse: Self = Self(5595);
    pub const ClientFriendUserStatusPublished: Self = Self(5596);
    pub const ClientCurrentUIMode: Self = Self(5597);
    pub const ClientVanityURLChangedNotification: Self = Self(5598);
    pub const ClientUserNotifications: Self = Self(5599);
    pub const BaseDFS: Self = Self(5600);
    pub const DFSGetFile: Self = Self(5601);
    pub const DFSInstallLocalFile: Self = Self(5602);
    pub const DFSConnection: Self = Self(5603);
    pub const DFSConnectionReply: Self = Self(5604);
    pub const ClientDFSAuthenticateRequest: Self = Self(5605);
    pub const ClientDFSAuthenticateResponse: Self = Self(5606);
    pub const ClientDFSEndSession: Self = Self(5607);
    pub const DFSPurgeFile: Self = Self(5608);
    pub const DFSRouteFile: Self = Self(5609);
    pub const DFSGetFileFromServer: Self = Self(5610);
    pub const DFSAcceptedResponse: Self = Self(5611);
    pub const DFSRequestPingback: Self = Self(5612);
    pub const DFSRecvTransmitFile: Self = Self(5613);
    pub const DFSSendTransmitFile: Self = Self(5614);
    pub const DFSRequestPingback2: Self = Self(5615);
    pub const DFSResponsePingback2: Self = Self(5616);
    pub const ClientDFSDownloadStatus: Self = Self(5617);
    pub const DFSStartTransfer: Self = Self(5618);
    pub const DFSTransferComplete: Self = Self(5619);
    pub const DFSRouteFileResponse: Self = Self(5620);
    pub const ClientNetworkingCertRequest: Self = Self(5621);
    pub const ClientNetworkingCertRequestResponse: Self = Self(5622);
    pub const ClientChallengeRequest: Self = Self(5623);
    pub const ClientChallengeResponse: Self = Self(5624);
    pub const BadgeCraftedNotification: Self = Self(5625);
    pub const ClientNetworkingMobileCertRequest: Self = Self(5626);
    pub const ClientNetworkingMobileCertRequestResponse: Self = Self(5627);
    pub const BaseMDS: Self = Self(5800);
    pub const MDSGetDepotDecryptionKey: Self = Self(5812);
    pub const MDSGetDepotDecryptionKeyResponse: Self = Self(5813);
    pub const MDSContentServerConfigRequest: Self = Self(5827);
    pub const MDSContentServerConfig: Self = Self(5828);
    pub const MDSGetDepotManifest: Self = Self(5829);
    pub const MDSGetDepotManifestResponse: Self = Self(5830);
    pub const MDSGetDepotManifestChunk: Self = Self(5831);
    pub const MDSToCSFlushChunk: Self = Self(5844);
    pub const MDSMigrateChunk: Self = Self(5847);
    pub const MDSMigrateChunkResponse: Self = Self(5848);
    pub const MDSToCSFlushManifest: Self = Self(5849);
    pub const CSBase: Self = Self(6200);
    pub const CSPing: Self = Self(6201);
    pub const CSPingResponse: Self = Self(6202);
    pub const GMSBase: Self = Self(6400);
    pub const GMSGameServerReplicate: Self = Self(6401);
    pub const ClientGMSServerQuery: Self = Self(6403);
    pub const GMSClientServerQueryResponse: Self = Self(6404);
    pub const AMGMSGameServerUpdate: Self = Self(6405);
    pub const AMGMSGameServerRemove: Self = Self(6406);
    pub const GameServerOutOfDate: Self = Self(6407);
    pub const MMSBase: Self = Self(6600);
    pub const ClientMMSCreateLobby: Self = Self(6601);
    pub const ClientMMSCreateLobbyResponse: Self = Self(6602);
    pub const ClientMMSJoinLobby: Self = Self(6603);
    pub const ClientMMSJoinLobbyResponse: Self = Self(6604);
    pub const ClientMMSLeaveLobby: Self = Self(6605);
    pub const ClientMMSLeaveLobbyResponse: Self = Self(6606);
    pub const ClientMMSGetLobbyList: Self = Self(6607);
    pub const ClientMMSGetLobbyListResponse: Self = Self(6608);
    pub const ClientMMSSetLobbyData: Self = Self(6609);
    pub const ClientMMSSetLobbyDataResponse: Self = Self(6610);
    pub const ClientMMSGetLobbyData: Self = Self(6611);
    pub const ClientMMSLobbyData: Self = Self(6612);
    pub const ClientMMSSendLobbyChatMsg: Self = Self(6613);
    pub const ClientMMSLobbyChatMsg: Self = Self(6614);
    pub const ClientMMSSetLobbyOwner: Self = Self(6615);
    pub const ClientMMSSetLobbyOwnerResponse: Self = Self(6616);
    pub const ClientMMSSetLobbyGameServer: Self = Self(6617);
    pub const ClientMMSLobbyGameServerSet: Self = Self(6618);
    pub const ClientMMSUserJoinedLobby: Self = Self(6619);
    pub const ClientMMSUserLeftLobby: Self = Self(6620);
    pub const ClientMMSInviteToLobby: Self = Self(6621);
    pub const ClientMMSFlushFrenemyListCache: Self = Self(6622);
    pub const ClientMMSFlushFrenemyListCacheResponse: Self = Self(6623);
    pub const ClientMMSSetLobbyLinked: Self = Self(6624);
    pub const ClientMMSSetRatelimitPolicyOnClient: Self = Self(6625);
    pub const ClientMMSGetLobbyStatus: Self = Self(6626);
    pub const ClientMMSGetLobbyStatusResponse: Self = Self(6627);
    pub const MMSGetLobbyList: Self = Self(6628);
    pub const MMSGetLobbyListResponse: Self = Self(6629);
    pub const GameServerPolicyUpdate: Self = Self(6631);
    pub const NonStdMsgBase: Self = Self(6800);
    pub const NonStdMsgMemcached: Self = Self(6801);
    pub const NonStdMsgHTTPServer: Self = Self(6802);
    pub const NonStdMsgHTTPClient: Self = Self(6803);
    pub const NonStdMsgWGResponse: Self = Self(6804);
    pub const NonStdMsgPHPSimulator: Self = Self(6805);
    pub const NonStdMsgChase: Self = Self(6806);
    pub const NonStdMsgDFSTransfer: Self = Self(6807);
    pub const NonStdMsgTests: Self = Self(6808);
    pub const NonStdMsgUMQpipeAAPL: Self = Self(6809);
    pub const NonStdMsgSyslog: Self = Self(6810);
    pub const NonStdMsgSteam2Emulator: Self = Self(6812);
    pub const NonStdMsgRTMPServer: Self = Self(6813);
    pub const NonStdMsgWebSocket: Self = Self(6814);
    pub const NonStdMsgRedis: Self = Self(6815);
    pub const UDSBase: Self = Self(7000);
    pub const ClientUDSP2PSessionStarted: Self = Self(7001);
    pub const ClientUDSP2PSessionEnded: Self = Self(7002);
    pub const UDSRenderUserAuth: Self = Self(7003);
    pub const UDSRenderUserAuthResponse: Self = Self(7004);
    pub const ClientInviteToGame: Self = Self(7005);
    pub const UDSHasSession: Self = Self(7006);
    pub const UDSHasSessionResponse: Self = Self(7007);
    pub const MPASBase: Self = Self(7100);
    pub const MPASVacBanReset: Self = Self(7101);
    pub const KGSBase: Self = Self(7200);
    pub const UCMBase: Self = Self(7300);
    pub const ClientUCMAddScreenshot: Self = Self(7301);
    pub const ClientUCMAddScreenshotResponse: Self = Self(7302);
    pub const UCMResetCommunityContent: Self = Self(7307);
    pub const UCMResetCommunityContentResponse: Self = Self(7308);
    pub const ClientUCMDeleteScreenshot: Self = Self(7309);
    pub const ClientUCMDeleteScreenshotResponse: Self = Self(7310);
    pub const ClientUCMPublishFile: Self = Self(7311);
    pub const ClientUCMPublishFileResponse: Self = Self(7312);
    pub const ClientUCMDeletePublishedFile: Self = Self(7315);
    pub const ClientUCMDeletePublishedFileResponse: Self = Self(7316);
    pub const ClientUCMUpdatePublishedFile: Self = Self(7325);
    pub const ClientUCMUpdatePublishedFileResponse: Self = Self(7326);
    pub const UCMUpdatePublishedFile: Self = Self(7327);
    pub const UCMUpdatePublishedFileResponse: Self = Self(7328);
    pub const UCMUpdatePublishedFileStat: Self = Self(7331);
    pub const UCMReloadPublishedFile: Self = Self(7337);
    pub const UCMReloadUserFileListCaches: Self = Self(7338);
    pub const UCMPublishedFileReported: Self = Self(7339);
    pub const UCMPublishedFilePreviewAdd: Self = Self(7341);
    pub const UCMPublishedFilePreviewAddResponse: Self = Self(7342);
    pub const UCMPublishedFilePreviewRemove: Self = Self(7343);
    pub const UCMPublishedFilePreviewRemoveResponse: Self = Self(7344);
    pub const UCMPublishedFileSubscribed: Self = Self(7349);
    pub const UCMPublishedFileUnsubscribed: Self = Self(7350);
    pub const UCMPublishFile: Self = Self(7351);
    pub const UCMPublishFileResponse: Self = Self(7352);
    pub const UCMPublishedFileChildAdd: Self = Self(7353);
    pub const UCMPublishedFileChildAddResponse: Self = Self(7354);
    pub const UCMPublishedFileChildRemove: Self = Self(7355);
    pub const UCMPublishedFileChildRemoveResponse: Self = Self(7356);
    pub const UCMPublishedFileParentChanged: Self = Self(7359);
    pub const ClientUCMSetUserPublishedFileAction: Self = Self(7364);
    pub const ClientUCMSetUserPublishedFileActionResponse: Self = Self(7365);
    pub const ClientUCMEnumeratePublishedFilesByUserAction: Self = Self(7366);
    pub const ClientUCMEnumeratePublishedFilesByUserActionResponse: Self = Self(7367);
    pub const UCMGetUserSubscribedFiles: Self = Self(7369);
    pub const UCMGetUserSubscribedFilesResponse: Self = Self(7370);
    pub const UCMFixStatsPublishedFile: Self = Self(7371);
    pub const ClientUCMEnumerateUserSubscribedFilesWithUpdates: Self = Self(7378);
    pub const ClientUCMEnumerateUserSubscribedFilesWithUpdatesResponse: Self = Self(7379);
    pub const UCMPublishedFileContentUpdated: Self = Self(7380);
    pub const ClientUCMPublishedFileUpdated: Self = Self(7381);
    pub const FSBase: Self = Self(7500);
    pub const ClientRichPresenceUpload: Self = Self(7501);
    pub const ClientRichPresenceRequest: Self = Self(7502);
    pub const ClientRichPresenceInfo: Self = Self(7503);
    pub const FSRichPresenceRequest: Self = Self(7504);
    pub const FSRichPresenceResponse: Self = Self(7505);
    pub const FSComputeFrenematrix: Self = Self(7506);
    pub const FSComputeFrenematrixResponse: Self = Self(7507);
    pub const FSPlayStatusNotification: Self = Self(7508);
    pub const FSAddOrRemoveFollower: Self = Self(7510);
    pub const FSAddOrRemoveFollowerResponse: Self = Self(7511);
    pub const FSUpdateFollowingList: Self = Self(7512);
    pub const FSCommentNotification: Self = Self(7513);
    pub const FSCommentNotificationViewed: Self = Self(7514);
    pub const ClientFSGetFollowerCount: Self = Self(7515);
    pub const ClientFSGetFollowerCountResponse: Self = Self(7516);
    pub const ClientFSGetIsFollowing: Self = Self(7517);
    pub const ClientFSGetIsFollowingResponse: Self = Self(7518);
    pub const ClientFSEnumerateFollowingList: Self = Self(7519);
    pub const ClientFSEnumerateFollowingListResponse: Self = Self(7520);
    pub const FSGetPendingNotificationCount: Self = Self(7521);
    pub const FSGetPendingNotificationCountResponse: Self = Self(7522);
    pub const ClientChatOfflineMessageNotification: Self = Self(7523);
    pub const ClientChatRequestOfflineMessageCount: Self = Self(7524);
    pub const ClientChatGetFriendMessageHistory: Self = Self(7525);
    pub const ClientChatGetFriendMessageHistoryResponse: Self = Self(7526);
    pub const ClientChatGetFriendMessageHistoryForOfflineMessages: Self = Self(7527);
    pub const ClientFSGetFriendsSteamLevels: Self = Self(7528);
    pub const ClientFSGetFriendsSteamLevelsResponse: Self = Self(7529);
    pub const AMRequestFriendData: Self = Self(7530);
    pub const DRMRange2: Self = Self(7600);
    pub const CEGVersionSetEnableDisableRequest: Self = Self(7600);
    pub const CEGVersionSetEnableDisableResponse: Self = Self(7601);
    pub const CEGPropStatusDRMSRequest: Self = Self(7602);
    pub const CEGPropStatusDRMSResponse: Self = Self(7603);
    pub const CEGWhackFailureReportRequest: Self = Self(7604);
    pub const CEGWhackFailureReportResponse: Self = Self(7605);
    pub const DRMSFetchVersionSet: Self = Self(7606);
    pub const DRMSFetchVersionSetResponse: Self = Self(7607);
    pub const EconBase: Self = Self(7700);
    pub const EconTrading_InitiateTradeRequest: Self = Self(7701);
    pub const EconTrading_InitiateTradeProposed: Self = Self(7702);
    pub const EconTrading_InitiateTradeResponse: Self = Self(7703);
    pub const EconTrading_InitiateTradeResult: Self = Self(7704);
    pub const EconTrading_StartSession: Self = Self(7705);
    pub const EconTrading_CancelTradeRequest: Self = Self(7706);
    pub const EconFlushInventoryCache: Self = Self(7707);
    pub const EconFlushInventoryCacheResponse: Self = Self(7708);
    pub const EconCDKeyProcessTransaction: Self = Self(7711);
    pub const EconCDKeyProcessTransactionResponse: Self = Self(7712);
    pub const EconGetErrorLogs: Self = Self(7713);
    pub const EconGetErrorLogsResponse: Self = Self(7714);
    pub const RMRange: Self = Self(7800);
    pub const RMTestVerisignOTP: Self = Self(7800);
    pub const RMTestVerisignOTPResponse: Self = Self(7801);
    pub const RMDeleteMemcachedKeys: Self = Self(7803);
    pub const RMRemoteInvoke: Self = Self(7804);
    pub const BadLoginIPList: Self = Self(7805);
    pub const RMMsgTraceAddOrUpdateTrigger: Self = Self(7806);
    pub const RMMsgTraceRemoveTriggers: Self = Self(7807);
    pub const RMMsgTraceEvent: Self = Self(7808);
    pub const UGSBase: Self = Self(7900);
    pub const UGSUpdateGlobalStats: Self = Self(7900);
    pub const ClientUGSGetGlobalStats: Self = Self(7901);
    pub const ClientUGSGetGlobalStatsResponse: Self = Self(7902);
    pub const StoreBase: Self = Self(8000);
    pub const UMQBase: Self = Self(8100);
    pub const WorkshopBase: Self = Self(8200);
    pub const WebAPIBase: Self = Self(8300);
    pub const WebAPIValidateOAuth2Token: Self = Self(8300);
    pub const WebAPIValidateOAuth2TokenResponse: Self = Self(8301);
    pub const WebAPIRegisterGCInterfaces: Self = Self(8303);
    pub const WebAPIInvalidateOAuthClientCache: Self = Self(8304);
    pub const WebAPIInvalidateOAuthTokenCache: Self = Self(8305);
    pub const WebAPISetSecrets: Self = Self(8306);
    pub const BackpackBase: Self = Self(8400);
    pub const BackpackAddToCurrency: Self = Self(8401);
    pub const BackpackAddToCurrencyResponse: Self = Self(8402);
    pub const CREBase: Self = Self(8500);
    pub const CREItemVoteSummary: Self = Self(8503);
    pub const CREItemVoteSummaryResponse: Self = Self(8504);
    pub const CREUpdateUserPublishedItemVote: Self = Self(8507);
    pub const CREUpdateUserPublishedItemVoteResponse: Self = Self(8508);
    pub const CREGetUserPublishedItemVoteDetails: Self = Self(8509);
    pub const CREGetUserPublishedItemVoteDetailsResponse: Self = Self(8510);
    pub const SecretsBase: Self = Self(8600);
    pub const SecretsRequestCredentialPair: Self = Self(8600);
    pub const SecretsCredentialPairResponse: Self = Self(8601);
    pub const BoxMonitorBase: Self = Self(8700);
    pub const BoxMonitorReportRequest: Self = Self(8700);
    pub const BoxMonitorReportResponse: Self = Self(8701);
    pub const PICSBase: Self = Self(8900);
    pub const ClientPICSChangesSinceRequest: Self = Self(8901);
    pub const ClientPICSChangesSinceResponse: Self = Self(8902);
    pub const ClientPICSProductInfoRequest: Self = Self(8903);
    pub const ClientPICSProductInfoResponse: Self = Self(8904);
    pub const ClientPICSAccessTokenRequest: Self = Self(8905);
    pub const ClientPICSAccessTokenResponse: Self = Self(8906);
    pub const ClientPICSPrivateBetaRequest: Self = Self(8907);
    pub const ClientPICSPrivateBetaResponse: Self = Self(8908);
    pub const WorkerProcess: Self = Self(9000);
    pub const WorkerProcessPingRequest: Self = Self(9000);
    pub const WorkerProcessPingResponse: Self = Self(9001);
    pub const WorkerProcessShutdown: Self = Self(9002);
    pub const DRMWorkerProcess: Self = Self(9100);
    pub const DRMWorkerProcessDRMAndSign: Self = Self(9100);
    pub const DRMWorkerProcessDRMAndSignResponse: Self = Self(9101);
    pub const DRMWorkerProcessSteamworksInfoRequest: Self = Self(9102);
    pub const DRMWorkerProcessSteamworksInfoResponse: Self = Self(9103);
    pub const DRMWorkerProcessInstallDRMDLLRequest: Self = Self(9104);
    pub const DRMWorkerProcessInstallDRMDLLResponse: Self = Self(9105);
    pub const DRMWorkerProcessSecretIdStringRequest: Self = Self(9106);
    pub const DRMWorkerProcessSecretIdStringResponse: Self = Self(9107);
    pub const DRMWorkerProcessInstallProcessedFilesRequest: Self = Self(9110);
    pub const DRMWorkerProcessInstallProcessedFilesResponse: Self = Self(9111);
    pub const DRMWorkerProcessExamineBlobRequest: Self = Self(9112);
    pub const DRMWorkerProcessExamineBlobResponse: Self = Self(9113);
    pub const DRMWorkerProcessDescribeSecretRequest: Self = Self(9114);
    pub const DRMWorkerProcessDescribeSecretResponse: Self = Self(9115);
    pub const DRMWorkerProcessBackfillOriginalRequest: Self = Self(9116);
    pub const DRMWorkerProcessBackfillOriginalResponse: Self = Self(9117);
    pub const DRMWorkerProcessValidateDRMDLLRequest: Self = Self(9118);
    pub const DRMWorkerProcessValidateDRMDLLResponse: Self = Self(9119);
    pub const DRMWorkerProcessValidateFileRequest: Self = Self(9120);
    pub const DRMWorkerProcessValidateFileResponse: Self = Self(9121);
    pub const DRMWorkerProcessSplitAndInstallRequest: Self = Self(9122);
    pub const DRMWorkerProcessSplitAndInstallResponse: Self = Self(9123);
    pub const DRMWorkerProcessGetBlobRequest: Self = Self(9124);
    pub const DRMWorkerProcessGetBlobResponse: Self = Self(9125);
    pub const DRMWorkerProcessEvaluateCrashRequest: Self = Self(9126);
    pub const DRMWorkerProcessEvaluateCrashResponse: Self = Self(9127);
    pub const DRMWorkerProcessAnalyzeFileRequest: Self = Self(9128);
    pub const DRMWorkerProcessAnalyzeFileResponse: Self = Self(9129);
    pub const DRMWorkerProcessUnpackBlobRequest: Self = Self(9130);
    pub const DRMWorkerProcessUnpackBlobResponse: Self = Self(9131);
    pub const DRMWorkerProcessInstallAllRequest: Self = Self(9132);
    pub const DRMWorkerProcessInstallAllResponse: Self = Self(9133);
    pub const DRMWorkerProcessSignFile: Self = Self(9134);
    pub const DRMWorkerProcessSignFileResponse: Self = Self(9135);
    pub const TestWorkerProcess: Self = Self(9200);
    pub const TestWorkerProcessLoadUnloadModuleRequest: Self = Self(9200);
    pub const TestWorkerProcessLoadUnloadModuleResponse: Self = Self(9201);
    pub const TestWorkerProcessServiceModuleCallRequest: Self = Self(9202);
    pub const TestWorkerProcessServiceModuleCallResponse: Self = Self(9203);
    pub const QuestServerBase: Self = Self(9300);
    pub const ClientGetEmoticonList: Self = Self(9330);
    pub const ClientEmoticonList: Self = Self(9331);
    pub const SLCBase: Self = Self(9400);
    pub const ClientSharedLibraryLockStatus: Self = Self(9405);
    pub const ClientSharedLibraryStopPlaying: Self = Self(9406);
    pub const RemoteClientBase: Self = Self(9500);
    pub const RemoteClientAuth_OBSOLETE: Self = Self(9500);
    pub const RemoteClientAuthResponse_OBSOLETE: Self = Self(9501);
    pub const RemoteClientAppStatus: Self = Self(9502);
    pub const RemoteClientStartStream: Self = Self(9503);
    pub const RemoteClientStartStreamResponse: Self = Self(9504);
    pub const RemoteClientPing: Self = Self(9505);
    pub const RemoteClientPingResponse: Self = Self(9506);
    pub const ClientUnlockH264: Self = Self(9507);
    pub const ClientUnlockH264Response: Self = Self(9508);
    pub const RemoteClientAcceptEULA: Self = Self(9509);
    pub const RemoteClientGetControllerConfig: Self = Self(9510);
    pub const RemoteClientGetControllerConfigResponse: Self = Self(9511);
    pub const RemoteClientStreamingEnabled: Self = Self(9512);
    pub const ClientUnlockHEVC_OBSOLETE: Self = Self(9513);
    pub const ClientUnlockHEVCResponse_OBSOLETE: Self = Self(9514);
    pub const RemoteClientStatusRequest: Self = Self(9515);
    pub const RemoteClientStatusResponse: Self = Self(9516);
    pub const RemoteClientAuthorizationRequest: Self = Self(9517);
    pub const RemoteClientAuthorizationResponse: Self = Self(9518);
    pub const RemoteClientAuthorizationCancelRequest: Self = Self(9519);
    pub const RemoteClientAuthorizationConfirmed: Self = Self(9520);
    pub const RemoteClientProofRequest: Self = Self(9521);
    pub const RemoteClientProofResponse: Self = Self(9522);
    pub const RemoteClientWifiAPStatus: Self = Self(9523);
    pub const ClientConcurrentSessionsBase: Self = Self(9600);
    pub const ClientPlayingSessionState: Self = Self(9600);
    pub const ClientKickPlayingSession: Self = Self(9601);
    pub const ClientBroadcastBase: Self = Self(9700);
    pub const ClientBroadcastInit: Self = Self(9700);
    pub const ClientBroadcastFrames: Self = Self(9701);
    pub const ClientBroadcastDisconnect: Self = Self(9702);
    pub const ClientBroadcastUploadConfig: Self = Self(9704);
    pub const BaseClient3: Self = Self(9800);
    pub const ClientVoiceCallPreAuthorize: Self = Self(9800);
    pub const ClientVoiceCallPreAuthorizeResponse: Self = Self(9801);
    pub const ClientServerTimestampRequest: Self = Self(9802);
    pub const ClientServerTimestampResponse: Self = Self(9803);
    pub const ServiceMethodCallFromClientNonAuthed: Self = Self(9804);
    pub const ClientHello: Self = Self(9805);
    pub const ClientEnableOrDisableDownloads: Self = Self(9806);
    pub const ClientEnableOrDisableDownloadsResponse: Self = Self(9807);
    pub const ClientFeatureGroupInfo: Self = Self(9808);
    pub const ClientLANP2PBase: Self = Self(9900);
    pub const ClientLANP2PRequestChunk: Self = Self(9900);
    pub const ClientLANP2PRequestChunkResponse: Self = Self(9901);
    pub const ClientPeerChunkRequest: Self = Self(9902);
    pub const ClientPeerChunkResponse: Self = Self(9903);
    pub const ClientLANP2PMax: Self = Self(9999);
    pub const BaseWatchdogServer: Self = Self(10000);
    pub const NotifyWatchdog: Self = Self(10000);
    pub const ClientSiteLicenseBase: Self = Self(10100);
    pub const ClientSiteLicenseSiteInfoNotification: Self = Self(10100);
    pub const ClientSiteLicenseCheckout: Self = Self(10101);
    pub const ClientSiteLicenseCheckoutResponse: Self = Self(10102);
    pub const ClientSiteLicenseGetAvailableSeats: Self = Self(10103);
    pub const ClientSiteLicenseGetAvailableSeatsResponse: Self = Self(10104);
    pub const ClientSiteLicenseGetContentCacheInfo: Self = Self(10105);
    pub const ClientSiteLicenseGetContentCacheInfoResponse: Self = Self(10106);
    pub const BaseChatServer: Self = Self(12000);
    pub const ChatServerGetPendingNotificationCount: Self = Self(12000);
    pub const ChatServerGetPendingNotificationCountResponse: Self = Self(12001);
    pub const BaseSecretServer: Self = Self(12100);
    pub const ServerSecretChanged: Self = Self(12100);
    pub const BaseWG: Self = Self(12200);
    pub const WGConnectionProtocolError: Self = Self(12200);
    pub const WGConnectionValidateUserToken: Self = Self(12201);
    pub const WGConnectionValidateUserTokenResponse: Self = Self(12202);
    pub const WGConnectionLegacyWGRequest: Self = Self(12203);
    pub const WGConnectionLegacyWGResponse: Self = Self(12204);
    pub const ClientPendingGameLaunch: Self = Self(12300);
    pub const ClientPendingGameLaunchResponse: Self = Self(12301);
}

bitflags! {
    /// Source: `enum EAppState` - used as a bitmask.
    #[repr(transparent)]
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct EAppState: u32 {
        const Invalid            = 0x0000_0000;
        const Uninstalled        = 0x0000_0001;
        const UpdateRequired     = 0x0000_0002;
        const FullyInstalled     = 0x0000_0004;
        const UpdateQueued       = 0x0000_0008;
        const UpdateOptional     = 0x0000_0010;
        const FilesMissing       = 0x0000_0020;
        const SharedOnly         = 0x0000_0040;
        const FilesCorrupt       = 0x0000_0080;
        const UpdateRunning      = 0x0000_0100;
        const UpdatePaused       = 0x0000_0200;
        const UpdateStarted      = 0x0000_0400;
        const Uninstalling       = 0x0000_0800;
        const BackupRunning      = 0x0000_1000;
        const AppRunning         = 0x0000_2000;
        const ComponentInUse     = 0x0000_4000;
        const MovingFolder       = 0x0000_8000;
        const Terminating        = 0x0001_0000;
        const PrefetchingInfo    = 0x0002_0000;
        const PeerServer         = 0x0004_0000;
        const UpdateDisabled     = 0x0008_0000;
        const Reconfiguring      = 0x0010_0000;
        const VerifyingInstalled = 0x0020_0000;
        const Preallocating      = 0x0040_0000;
        const Downloading        = 0x0080_0000;
        const Staging            = 0x0100_0000;
        const Committing         = 0x0200_0000;
        const VerifyingStaged    = 0x0400_0000;
        const Stopping           = 0x0800_0000;
    }
}

#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ERegistrySubTree {
    News = 0,
    Apps = 1,
    Subscriptions = 2,
    GameServers = 3,
    Friends = 4,
    System = 5,
    AppOwnershipTickets = 6,
    LegacyCDKeys = 9,
    ClientConfig = 10,
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EKeyValuesType {
    None = 0,
    String = 1,
    Int = 2,
    Float = 3,
    Ptr = 4,
    WString = 5,
    Color = 6,
    UInt64 = 7,
    Chained = 9,
    Int64 = 10,
}

/// Source: `enum EWebSocketOpCode : char` (1 byte signed on MSVC, but all
/// values are non-negative). Modelled as u8 for simplicity.
#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EWebSocketOpCode {
    Continuation = 0x00,
    Text = 0x01,
    Binary = 0x02,
    Close = 0x08,
    Ping = 0x09,
    Pong = 0x0A,
}

bitflags! {
    /// Source: `enum class EAppChangeFlags`.
    #[repr(transparent)]
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct EAppChangeFlags: u32 {
        const AddedOrCreated          = 0x0001;
        const AppInfoOrConfig         = 0x0002;
        const ClientShutdown          = 0x0004;
        const UpdateProgress          = 0x0010;
        const InternalRunState        = 0x0040;
        const CloudState              = 0x0100;
        const LogonState              = 0x0200;
        const RemoteAppState          = 0x0800;
        const RemoteLibraryAssetState = 0x1000;
        const GameAction              = 0x2000;
        const LibraryAssetCleanup     = 0x4000;
        const MRURegenerated          = 0x8000;
    }
}

#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EAccountType {
    Invalid = 0,
    Individual = 1,
    Multiseat = 2,
    GameServer = 3,
    AnonGameServer = 4,
    Pending = 5,
    ContentServer = 6,
    Clan = 7,
    Chat = 8,
    ConsoleUser = 9,
    AnonUser = 10,
    Max = 11,
}

#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EUniverse {
    Invalid = 0,
    Public = 1,
    Beta = 2,
    Internal = 3,
    Dev = 4,
    Max = 5,
}

#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EConfigStore {
    Invalid = 0,
    Install = 1,
    UserRoaming = 2,
    UserLocal = 3,
    Max = 4,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn e_result_spot_values() {
        assert_eq!(EResult::OK as i32, 1);
        assert_eq!(EResult::Fail as i32, 2);
        assert_eq!(EResult::AccessDenied as i32, 15);
        assert_eq!(EResult::InvalidParam as i32, 8);
        assert_eq!(EResult::AccountLogonDenied as i32, 63);
        assert_eq!(EResult::OfflineAppCacheInvalid as i32, 130);
        assert_eq!(size_of::<EResult>(), 4);
    }

    #[test]
    fn e_msg_alias_values() {
        // The two aliased values from the C++ header.
        assert_eq!(EMsg::BaseGeneral, EMsg::GenericReply);
        assert_eq!(EMsg::BaseShell, EMsg::AssignSysID);
        assert_eq!(EMsg::BaseGM, EMsg::Heartbeat);
        // Spot-check specific values used by the codebase.
        assert_eq!(EMsg::Multi.0, 1);
        assert_eq!(EMsg::ServiceMethodResponse.0, 147);
        assert_eq!(EMsg::ServiceMethodCallFromClient.0, 151);
        assert_eq!(EMsg::ClientGetLegacyGameKey.0, 730);
        assert_eq!(EMsg::ClientGamesPlayed.0, 742);
        assert_eq!(EMsg::ClientPersonaState.0, 766);
        assert_eq!(EMsg::ClientLicenseList.0, 780);
        assert_eq!(EMsg::ClientGetLegacyGameKeyResponse.0, 785);
        assert_eq!(EMsg::ClientGetUserStats.0, 818);
        assert_eq!(EMsg::ClientGetUserStatsResponse.0, 819);
        assert_eq!(EMsg::ClientGetAppOwnershipTicketResponse.0, 858);
        assert_eq!(EMsg::ClientGamesPlayedWithDataBlob.0, 5410);
        assert_eq!(EMsg::ClientRichPresenceUpload.0, 7501);
        assert_eq!(EMsg::ClientPICSProductInfoRequest.0, 8903);
        assert_eq!(EMsg::ClientSharedLibraryStopPlaying.0, 9406);
    }

    #[test]
    fn ownership_flags_combine() {
        let f = EAppOwnershipFlags::OwnsLicense | EAppOwnershipFlags::FreeLicense;
        assert_eq!(f.bits(), 0x0003);
        assert!(f.contains(EAppOwnershipFlags::OwnsLicense));
        assert!(!f.contains(EAppOwnershipFlags::RetailLicense));
    }

    #[test]
    fn app_state_combine() {
        let s = EAppState::UpdateRunning | EAppState::UpdateStarted;
        assert_eq!(s.bits(), 0x0500);
        assert!(s.contains(EAppState::UpdateRunning));
    }

    #[test]
    fn websocket_opcode_layout() {
        assert_eq!(EWebSocketOpCode::Binary as u8, 0x02);
        assert_eq!(size_of::<EWebSocketOpCode>(), 1);
    }

    #[test]
    fn account_type_and_universe_layout() {
        assert_eq!(EAccountType::Individual as i32, 1);
        assert_eq!(EAccountType::Max as i32, 11);
        assert_eq!(EUniverse::Public as i32, 1);
        assert_eq!(EUniverse::Max as i32, 5);
    }

    #[test]
    fn key_values_type_layout() {
        assert_eq!(EKeyValuesType::String as u32, 1);
        assert_eq!(EKeyValuesType::UInt64 as u32, 7);
        assert_eq!(size_of::<EKeyValuesType>(), 4);
    }
}
