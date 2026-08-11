//! 프로토콜 §6.5 종료코드 / §6.6 에러코드에 대응하는 에러 타입.

use std::fmt;

/// 프로토콜 §6.5의 종료코드.
///
/// `6`(idle timeout)은 호스트가 부과하는 코드이므로 플러그인은 절대 내지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    Success = 0,
    /// 입력 파일이 손상됨.
    CorruptInput = 2,
    /// 이 빌드에서 지원하지 않는 기능.
    UnsupportedFeature = 3,
    /// 프로토콜 버전 불일치.
    ProtocolMismatch = 5,
    /// 문서화되지 않은 코드. 메인은 `internal_error`로 보고한다.
    InternalError = 70, // sysexits.h EX_SOFTWARE
}

impl ExitCode {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

/// 프로토콜 §6.6의 `error.code` 문자열.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidArgument,
    UnsupportedCommand,
    UnsupportedFeature,
    CorruptInput,
    InternalError,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidArgument => "invalid_argument",
            Self::UnsupportedCommand => "unsupported_command",
            Self::UnsupportedFeature => "unsupported_feature",
            Self::CorruptInput => "corrupt_input",
            Self::InternalError => "internal_error",
        }
    }
}

#[derive(Debug)]
pub struct PluginError {
    pub code: ErrorCode,
    pub message: String,
}

impl PluginError {
    pub fn corrupt(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::CorruptInput,
            message: message.into(),
        }
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::InvalidArgument,
            message: message.into(),
        }
    }

    pub fn unsupported_command(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::UnsupportedCommand,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::InternalError,
            message: message.into(),
        }
    }

    pub fn exit_code(&self) -> ExitCode {
        match self.code {
            ErrorCode::CorruptInput => ExitCode::CorruptInput,
            ErrorCode::UnsupportedFeature => ExitCode::UnsupportedFeature,
            // 잘못된 인자와 미지원 명령은 문서화된 전용 코드가 없다.
            // §6.5의 "other → internal_error" 규칙을 따른다.
            ErrorCode::InvalidArgument
            | ErrorCode::UnsupportedCommand
            | ErrorCode::InternalError => ExitCode::InternalError,
        }
    }
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for PluginError {}

impl From<std::io::Error> for PluginError {
    fn from(e: std::io::Error) -> Self {
        // 파일을 못 읽는 것은 소스가 손상/부재한 경우이므로 corrupt_input으로 본다.
        Self::corrupt(format!("io error: {e}"))
    }
}

impl From<zip::result::ZipError> for PluginError {
    fn from(e: zip::result::ZipError) -> Self {
        Self::corrupt(format!("not a readable zip container: {e}"))
    }
}

impl From<quick_xml::Error> for PluginError {
    fn from(e: quick_xml::Error) -> Self {
        Self::corrupt(format!("malformed xml: {e}"))
    }
}

impl From<quick_xml::events::attributes::AttrError> for PluginError {
    fn from(e: quick_xml::events::attributes::AttrError) -> Self {
        Self::corrupt(format!("malformed xml attribute: {e}"))
    }
}

impl From<quick_xml::encoding::EncodingError> for PluginError {
    fn from(e: quick_xml::encoding::EncodingError) -> Self {
        Self::corrupt(format!("undecodable text encoding: {e}"))
    }
}

pub type Result<T> = std::result::Result<T, PluginError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_input_maps_to_exit_two() {
        assert_eq!(
            PluginError::corrupt("x").exit_code(),
            ExitCode::CorruptInput
        );
        assert_eq!(ExitCode::CorruptInput.as_i32(), 2);
    }

    #[test]
    fn exit_codes_match_protocol_table() {
        // docs/01-protocol-contract.md C5
        assert_eq!(ExitCode::Success.as_i32(), 0);
        assert_eq!(ExitCode::CorruptInput.as_i32(), 2);
        assert_eq!(ExitCode::UnsupportedFeature.as_i32(), 3);
        assert_eq!(ExitCode::ProtocolMismatch.as_i32(), 5);
    }

    #[test]
    fn plugin_never_emits_idle_timeout_code_six() {
        // §6.5: 6은 호스트가 부과하는 코드다. 우리 열거형에 6이 있으면 안 된다.
        for c in [
            ExitCode::Success,
            ExitCode::CorruptInput,
            ExitCode::UnsupportedFeature,
            ExitCode::ProtocolMismatch,
            ExitCode::InternalError,
        ] {
            assert_ne!(c.as_i32(), 6, "{c:?} must not be 6");
        }
    }

    #[test]
    fn error_codes_use_snake_case() {
        // §5.5: 모든 JSON 키/값은 snake_case
        for c in [
            ErrorCode::InvalidArgument,
            ErrorCode::UnsupportedCommand,
            ErrorCode::UnsupportedFeature,
            ErrorCode::CorruptInput,
            ErrorCode::InternalError,
        ] {
            let s = c.as_str();
            assert!(
                s.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_'),
                "{s} is not snake_case"
            );
        }
    }
}
