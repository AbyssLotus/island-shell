//! The Focus timer is all UI (ui/focus.js, its rules in ui/logic.js, tested by
//! dev/tests/focus.test.js); this crate is here so the workspace checks its manifest like every
//! other module's.

#[cfg(test)]
mod tests {
    #[test]
    fn manifest_is_valid() {
        let m: island_module_proto::manifest::Manifest = serde_json::from_str(include_str!("../module.json")).unwrap();
        m.validate().unwrap();
        assert_eq!(m.id, "focus");
        assert!(m.backend.is_none());
        assert_eq!(m.contributes.sections[0].replaces.as_deref(), Some("focus"));
    }
}
