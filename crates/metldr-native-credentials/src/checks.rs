use super::*;

#[test]
fn ids_and_keys_are_strict_and_errors_are_redacted() {
    assert!(CredentialId::parse("0123456789abcdef0123456789abcdef").is_ok());
    assert!(CredentialId::parse("0123456789ABCDEF0123456789ABCDEF").is_err());
    assert!(Key::parse("has space".to_owned()).is_err());
    assert_eq!(Error::Invalid.to_string(), "invalid credential input");
}

#[test]
fn malformed_stored_bytes_are_corrupt() {
    assert!(matches!(key_from_bytes(vec![b'a', 0]), Err(Error::Corrupt)));
    assert!(matches!(key_from_bytes(Vec::new()), Err(Error::Corrupt)));
}

#[test]
fn generated_ids_have_fixed_lowercase_shape() {
    let id = CredentialId::generate().unwrap();
    assert_eq!(id.as_str().len(), 32);
    assert!(
        id.as_str()
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
}

#[test]
#[ignore = "requires an isolated owned Secret Service"]
fn owned_partial_records_fail_closed() {
    assert_eq!(
        std::env::var("METLDR_OWNED_VAULT_CHECK").unwrap(),
        "isolated-dummy-only"
    );
    let scope = CredentialId::generate().unwrap();
    let id = CredentialId::generate().unwrap();
    let vault = Vault::new(scope.clone());
    backend::put(&scope, &id, "tail", b"old-tail").unwrap();
    assert!(matches!(
        vault.put(&id, &Key::parse("short".into()).unwrap()),
        Err(Error::Conflict)
    ));
    assert!(matches!(vault.read(&id), Err(Error::Missing)));
    assert!(backend::read(&scope, &id, "head").unwrap().is_none());
    vault.remove(&id).unwrap();
    assert!(backend::read(&scope, &id, "tail").unwrap().is_none());
    let mut incomplete = vec![1, 2];
    incomplete.extend_from_slice(&vec![b'K'; PART_SIZE]);
    backend::put(&scope, &id, "head", &incomplete).unwrap();
    assert!(matches!(vault.read(&id), Err(Error::Corrupt)));
    vault.remove(&id).unwrap();
    backend::put(&scope, &id, "head", b"malformed").unwrap();
    assert!(matches!(vault.read(&id), Err(Error::Corrupt)));
    vault.remove(&id).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|value| {
            let (scope, id, barrier) = (scope.clone(), id.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                (
                    value,
                    Vault::new(scope).put(&id, &Key::parse(value.into()).unwrap()),
                )
            })
        })
        .collect();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(
        results.iter().filter(|(_, result)| result.is_ok()).count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|(_, result)| matches!(result, Err(Error::Conflict)))
            .count(),
        1
    );
    assert!(
        vault.read(&id).unwrap().expose()
            == results.iter().find(|(_, result)| result.is_ok()).unwrap().0
    );
    vault.remove(&id).unwrap();
}
