#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StdErrorClass {
    Std,
    Io,
    Fs,
    FsNotFound,
    FsPermission,
    FsExists,
    FsEncoding,
    BadDescriptor,
    Closed,
    Net,
    Resolve,
    Connect,
    Tls,
    Protocol,
    Capability,
    Timeout,
    Cancelled,
    Parse,
    Decode,
    Sql,
    SqlConstraint,
}

impl StdErrorClass {
    pub const MODULE: &'static str = "std:error";

    pub const ALL: [Self; 21] = [
        Self::Std,
        Self::Io,
        Self::Fs,
        Self::FsNotFound,
        Self::FsPermission,
        Self::FsExists,
        Self::FsEncoding,
        Self::BadDescriptor,
        Self::Closed,
        Self::Net,
        Self::Resolve,
        Self::Connect,
        Self::Tls,
        Self::Protocol,
        Self::Capability,
        Self::Timeout,
        Self::Cancelled,
        Self::Parse,
        Self::Decode,
        Self::Sql,
        Self::SqlConstraint,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Std => "StdError",
            Self::Io => "IoError",
            Self::Fs => "FsError",
            Self::FsNotFound => "FsNotFoundError",
            Self::FsPermission => "FsPermissionError",
            Self::FsExists => "FsExistsError",
            Self::FsEncoding => "FsEncodingError",
            Self::BadDescriptor => "BadDescriptorError",
            Self::Closed => "ClosedError",
            Self::Net => "NetError",
            Self::Resolve => "ResolveError",
            Self::Connect => "ConnectError",
            Self::Tls => "TlsError",
            Self::Protocol => "ProtocolError",
            Self::Capability => "CapabilityError",
            Self::Timeout => "TimeoutError",
            Self::Cancelled => "CancelledError",
            Self::Parse => "ParseError",
            Self::Decode => "DecodeError",
            Self::Sql => "SqlError",
            Self::SqlConstraint => "SqlConstraintError",
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::Std => "std",
            Self::Io => "io",
            Self::Fs => "fs",
            Self::FsNotFound => "fs.not_found",
            Self::FsPermission => "fs.permission_denied",
            Self::FsExists => "fs.exists",
            Self::FsEncoding => "fs.encoding",
            Self::BadDescriptor => "io.bad_descriptor",
            Self::Closed => "io.closed",
            Self::Net => "net",
            Self::Resolve => "net.resolve",
            Self::Connect => "net.connect",
            Self::Tls => "net.tls",
            Self::Protocol => "net.protocol",
            Self::Capability => "capability",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::Parse => "parse",
            Self::Decode => "parse.decode",
            Self::Sql => "sql",
            Self::SqlConstraint => "sql.constraint",
        }
    }

    pub const fn parent(self) -> Option<Self> {
        match self {
            Self::Std => None,
            Self::Io
            | Self::Capability
            | Self::Timeout
            | Self::Cancelled
            | Self::Parse
            | Self::Sql => Some(Self::Std),
            Self::Fs | Self::BadDescriptor | Self::Closed | Self::Net => Some(Self::Io),
            Self::FsNotFound | Self::FsPermission | Self::FsExists | Self::FsEncoding => {
                Some(Self::Fs)
            }
            Self::Resolve | Self::Connect | Self::Tls | Self::Protocol => Some(Self::Net),
            Self::Decode => Some(Self::Parse),
            Self::SqlConstraint => Some(Self::Sql),
        }
    }

    pub fn of_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|class| class.name() == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorField {
    Code,
    Path,
    Host,
    Port,
    Capability,
    Target,
    Position,
    Sql,
}

impl ErrorField {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Path => "path",
            Self::Host => "host",
            Self::Port => "port",
            Self::Capability => "capability",
            Self::Target => "target",
            Self::Position => "position",
            Self::Sql => "sql",
        }
    }
}
