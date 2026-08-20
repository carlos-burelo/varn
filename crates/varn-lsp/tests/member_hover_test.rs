#![allow(unused_crate_dependencies)]

use varn_lsp::features::hover::build_hover;
use varn_lsp::pipeline::run_pipeline;

#[test]
fn test_member_and_receiver_hovers() {
    let source = r#"
const map: Map<int> = new Map<int>()
map.set("a", 1)
const mk: str[] = map.keys()

const arr2: int[] = (0..4).toArray()

const stepped: int[] = (0..10).step(3).toArray()
assert("step len", stepped.length === 4)

const r3: Range = Range.from(5, 10)
"#;
    let uri = "file:///test/members.vn";
    let state = run_pipeline(source.to_string(), uri.to_string());

    // 1. Hover on 'keys' in 'map.keys()' (line 3, col 23)
    let hover_keys = build_hover(&state, 3, 23).expect("Hover on keys should succeed");
    if let tower_lsp::lsp_types::HoverContents::Markup(mc) = hover_keys.contents {
        assert!(
            mc.value.contains("(method) Map") || mc.value.contains("(method) map.keys") || mc.value.contains("keys()"),
            "Unexpected hover for keys: {}",
            mc.value
        );
        assert!(!mc.value.contains("function map.keys"), "Should NOT say 'function map.keys': {}", mc.value);
    }

    // 2. Hover on 'toArray' in '(0..4).toArray()' (line 5, col 28)
    let hover_to_array = build_hover(&state, 5, 28).expect("Hover on toArray should succeed");
    if let tower_lsp::lsp_types::HoverContents::Markup(mc) = hover_to_array.contents {
        assert!(
            mc.value.contains("Range.toArray") || mc.value.contains("toArray()"),
            "Unexpected hover for toArray: {}",
            mc.value
        );
        assert!(!mc.value.contains("function ).toArray"), "Should NEVER have ')' in receiver name: {}", mc.value);
    }

    // 3. Hover on 'length' in 'stepped.length' (line 8, col 27)
    let hover_len = build_hover(&state, 8, 27).expect("Hover on length should succeed");
    if let tower_lsp::lsp_types::HoverContents::Markup(mc) = hover_len.contents {
        assert!(
            mc.value.contains("length: int"),
            "Unexpected hover for length: {}",
            mc.value
        );
        assert!(!mc.value.starts_with("```varn\nstepped.length"), "Should not just echo 'stepped.length': {}", mc.value);
    }

    // 4. Hover on 'from' in 'Range.from(5, 10)' (line 10, col 25)
    let hover_from = build_hover(&state, 10, 25).expect("Hover on from should succeed");
    if let tower_lsp::lsp_types::HoverContents::Markup(mc) = hover_from.contents {
        assert!(
            mc.value.contains("Range.from"),
            "Unexpected hover for from: {}",
            mc.value
        );
        assert!(!mc.value.contains("function Range.from"), "Should say '(static method) Range.from' or '(method)': {}", mc.value);
    }

    // 5. Member completions on Map
    let map_ty = varn_checker::Type::generic(
        "Map".to_string(),
        vec![varn_checker::Type::Int],
    );
    let map_completions = varn_lsp::features::completion::members::build_member_completions(
        &state,
        varn_lsp::features::completion::members::ReceiverInfo::Typed {
            ty: map_ty,
            is_instance: true,
        },
        false,
    );
    let map_comp_labels: Vec<String> = map_completions.into_iter().map(|c| c.label).collect();
    assert!(map_comp_labels.contains(&"set".to_string()), "Map completion missing 'set': {:?}", map_comp_labels);
    assert!(map_comp_labels.contains(&"get".to_string()), "Map completion missing 'get': {:?}", map_comp_labels);
    assert!(map_comp_labels.contains(&"size".to_string()), "Map completion missing 'size': {:?}", map_comp_labels);

    // 6. Member completions on Range
    let range_ty = varn_checker::Type::named("Range".to_string());
    let range_completions = varn_lsp::features::completion::members::build_member_completions(
        &state,
        varn_lsp::features::completion::members::ReceiverInfo::Typed {
            ty: range_ty,
            is_instance: true,
        },
        false,
    );
    let range_comp_labels: Vec<String> = range_completions.into_iter().map(|c| c.label).collect();
    assert!(range_comp_labels.contains(&"toArray".to_string()), "Range completion missing 'toArray': {:?}", range_comp_labels);
    assert!(range_comp_labels.contains(&"step".to_string()), "Range completion missing 'step': {:?}", range_comp_labels);

    // 7. Signature help on map.set("a", 1) (line 2, col 12)
    let sig_help = varn_lsp::features::signature_help::build_signature_help(&state, 2, 12);
    assert!(sig_help.is_some(), "Signature help for map.set should succeed");
    let sig = sig_help.unwrap();
    assert_eq!(sig.signatures.len(), 1);
    assert!(sig.signatures[0].label.contains("set("), "Expected set( in signature help: {}", sig.signatures[0].label);
}

