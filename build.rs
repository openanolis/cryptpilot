#[cfg(feature = "provider-kbs")]
use ttrpc_codegen::{Codegen, Customize, ProtobufCustomize};

fn main() -> shadow_rs::SdResult<()> {
    #[cfg(feature = "provider-kbs")]
    {
        // Build for connecting AA with ttrpc
        let protos = vec!["src/measure/attestation_agent/protos/attestation-agent.proto"];
        let protobuf_customized = ProtobufCustomize::default().gen_mod_rs(false);

        Codegen::new()
            .out_dir("src/measure/attestation_agent/ttrpc_protocol")
            .inputs(&protos)
            .include("src/measure/attestation_agent/protos")
            .rust_protobuf()
            .customize(Customize {
                async_all: true,
                ..Default::default()
            })
            .rust_protobuf_customize(protobuf_customized)
            .run()
            .expect("Generate ttrpc protocol code failed.");
    }
    shadow_rs::new()
}
