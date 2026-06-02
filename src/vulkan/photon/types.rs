//! # Structs and Enums used as abstraction layers for vulkan decoding pipeline.


use std::ffi::CStr;
pub struct DecodeExtensions;
use thiserror::Error;
impl DecodeExtensions {
    pub const H264: &'static CStr = c"VK_KHR_video_decode_h264";
    pub const H265: &'static CStr = c"VK_KHR_video_decode_h265";
    pub const AV1: &'static CStr = c"VK_KHR_video_decode_av1";
}


/// Describes which codecs are supported by the physical device.
pub struct SupportedCodecs {
    pub h264: bool,
    pub h265: bool,
    pub av1: bool,
}
impl Default for SupportedCodecs {
    fn default() -> Self {
        Self {
            h264: false,
            h265: false,
            av1: false,
        }
    }
}


/// Wrapper for profiles enums of vk::native.
#[allow(non_snake_case)]
pub mod VideoCodecsProfiles {
    use ash::vk;

    #[repr(u32)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum H264Profiles {
        Baseline = vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_BASELINE,
        Main = vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_MAIN,
        High = vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_HIGH,
        High444 = vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_HIGH_444_PREDICTIVE,
        Invalid = vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_INVALID,
    }
    
    #[repr(u32)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum H265Profiles {
        Main = vk::native::StdVideoH265ProfileIdc_STD_VIDEO_H265_PROFILE_IDC_MAIN,
        Main10 = vk::native::StdVideoH265ProfileIdc_STD_VIDEO_H265_PROFILE_IDC_MAIN_10,
        Invalid = vk::native::StdVideoH265ProfileIdc_STD_VIDEO_H265_PROFILE_IDC_INVALID,
    }
    
    #[repr(u32)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum AV1Profiles {
        Main = vk::native::StdVideoAV1Profile_STD_VIDEO_AV1_PROFILE_MAIN,
        High = vk::native::StdVideoAV1Profile_STD_VIDEO_AV1_PROFILE_HIGH,
        Professional = vk::native::StdVideoAV1Profile_STD_VIDEO_AV1_PROFILE_PROFESSIONAL,
        Invalid = vk::native::StdVideoAV1Profile_STD_VIDEO_AV1_PROFILE_INVALID,
    }

    #[repr(u32)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum UnknownProfile {
        Unknown = i32::MAX as u32,
    }
    
    pub enum UnifiedVideoProfile {
        H264(H264Profiles),
        H265(H265Profiles),
        AV1(AV1Profiles),
        Unknown(UnknownProfile),
    }
    



    
    pub trait VideoProfile {
        fn as_raw(&self) -> u32;
    }
    impl VideoProfile for H264Profiles {
        fn as_raw(&self) -> u32 {
            *self as u32
        }
    }
    
    impl VideoProfile for H265Profiles {
        fn as_raw(&self) -> u32 {
            *self as u32
        }
    }
    
    impl VideoProfile for AV1Profiles {
        fn as_raw(&self) -> u32 {
            *self as u32
        }
    }
    impl VideoProfile for UnknownProfile {
        fn as_raw(&self) -> u32 {
            *self as u32
        }
    }
    impl VideoProfile for UnifiedVideoProfile {
        fn as_raw(&self) -> u32 {
            match self {
                UnifiedVideoProfile::H264(profile) => profile.as_raw(),
                UnifiedVideoProfile::H265(profile) => profile.as_raw(),
                UnifiedVideoProfile::AV1(profile) => profile.as_raw(),
                UnifiedVideoProfile::Unknown(profile) => profile.as_raw(),
            }
        }
    }
}




/// Error handling
#[derive(Error, Debug)]
pub enum PhotonError {
    #[error("Can't find a valid video profile.")]
    NoProfileIndicator,
    #[error("Invalid codec operation.")]
    InvalidCodecOperation,
    #[error("Vulkan driver error: {0:?}")]
    VulkanError(#[from] ash::vk::Result),
}