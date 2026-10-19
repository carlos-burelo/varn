use super::class::ClassObj;
use super::RuntimeSymbol;
use crate::{NativeCtx, NativeFnResult, VmValue};

pub const CHANNEL_CLASS: &str = "Channel";
pub const CHANNEL_CLOSED_CLASS: &str = "ChannelClosed";
pub const ENDPOINT_ID_FIELD: &str = "_chan";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelEndpoint {
    Sender,
    Receiver,
}

impl ChannelEndpoint {
    const ALL: [Self; 2] = [Self::Sender, Self::Receiver];

    pub const fn class_name(self) -> &'static str {
        match self {
            Self::Sender => "Sender",
            Self::Receiver => "Receiver",
        }
    }

    pub fn of_class(class: &ClassObj) -> Option<Self> {
        if !class.is_native {
            return None;
        }
        Self::ALL.into_iter().find(|e| e.class_name() == class.name)
    }

    pub fn alloc(self, ctx: &mut dyn NativeCtx, id: u64) -> Result<VmValue, String> {
        let class = self.class_name();
        let endpoint = ctx
            .alloc_instance(class)
            .ok_or_else(|| format!("channel: {class} class not registered"))?;
        ctx.set_field(endpoint, ENDPOINT_ID_FIELD, VmValue::from_int(id as i64));
        if self == Self::Receiver {
            let iter = ctx.alloc_bound_native(endpoint, itself, RuntimeSymbol::AsyncIterator.method_name());
            ctx.set_field(endpoint, RuntimeSymbol::AsyncIterator.name(), iter);
        }
        Ok(endpoint)
    }
}

pub fn endpoint_id(ctx: &mut dyn NativeCtx, endpoint: VmValue) -> Option<u64> {
    let id = ctx.get_field(endpoint, ENDPOINT_ID_FIELD)?;
    id.is_int().then(|| id.as_int() as u64)
}

fn itself(_ctx: &mut dyn NativeCtx, args: &[VmValue]) -> NativeFnResult {
    args.first()
        .copied()
        .ok_or_else(|| "receiver iterator: missing self".into())
}
