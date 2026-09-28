//! Behavioral cases shared by identity and direct persistence tests.

pub fn encode(text: &str) -> String {
    text.bytes().map(|byte| format!("%{byte:02X}")).collect()
}

pub fn rejected() -> Vec<String> {
    let mut cases = Vec::new();
    // Independent credential families crossed with locations and encoding.
    for name in [
        "token",
        "ACCESS_token",
        "API-Key",
        "X-Amz-Signature",
        "X-Goog-Credential",
        "sig",
        "resourcekey",
        "password",
        "session_id",
    ] {
        let parameters = format!("{name}=TEST_SECRET");
        let nested = format!("https://example.org/file?{parameters}");
        for payload in [
            parameters.clone(),
            encode(&parameters),
            encode(&encode(&parameters)),
        ] {
            cases.push(format!("https://example.com/file?{payload}"));
            cases.push(format!("https://example.com/file#{payload}"));
            cases.push(format!("https://example.com/file#/section?{payload}"));
        }
        cases.push(format!("https://example.com/file#{parameters}?view=1"));
        for locator in [
            nested,
            format!("/file?{parameters}"),
            format!("//example.org/file?{parameters}"),
            format!("file?{parameters}"),
        ] {
            for payload in [locator.clone(), encode(&locator), encode(&encode(&locator))] {
                cases.push(format!("https://example.com/file?next={payload}"));
                cases.push(format!("https://example.com/file#next={payload}"));
            }
        }
        cases.push(format!(
            "https://docs.google.com/document/d/ABC/edit?{parameters}"
        ));
        cases.push(format!("file:///tmp/report?{parameters}"));
    }
    for nested in [
        "https://user:TEST_SECRET@example.org/file",
        "//user:TEST_SECRET@example.org/file",
        "https://user:TEST_SECRET@example.org/../../safe",
        "https://user:TEST_SECRET@example.org/%2e%2e/%2e%2e/safe",
        "https:\\user:TEST_SECRET@example.org/file",
        "ht\ttps://user:TEST_SECRET@example.org/file",
    ] {
        for payload in [nested.to_owned(), encode(nested), encode(&encode(nested))] {
            cases.push(format!("https://proxy.example/{payload}"));
            cases.push(format!("https://example.com/file#{payload}"));
            cases.push(format!("archive/{payload}"));
        }
    }
    for path in [
        "callback",
        "callback/.",
        "callback/sub/..",
        "oauth/return",
        "oauth2/return",
        "cb",
        "login",
    ] {
        cases.push(format!(
            "https://example.com/{path}?code=TEST_SECRET&state=abc"
        ));
        cases.push(format!("https://example.com/#{path}?code=TEST_SECRET"));
        cases.push(format!("https://example.com/{path}#code=TEST_SECRET"));
    }
    cases
}

pub fn accepted() -> Vec<String> {
    let mut cases = [
        "https://example.com/products?code=ABC&state=CA",
        "https://example.com/products?code=XYZ&state=CA",
        "https://example.com/docs/auth/examples?code=ABC",
        "https://example.com/list?page_token=next&continuation_token=abc",
        "https://example.com/file?empty=&q=why%3F&id=1",
        "https://example.com/docs#token",
        "https://example.com/docs#auth",
        "https://example.com/docs#signature",
        "https://example.com/docs#code",
        "https://example.com/file#section?code=ABC&state=CA",
        "https://example.com/callback#next=https://example.org/products?code=ABC&state=CA",
        "https://example.com/https://example.org/../../safe",
        "https://example.com/caf%C3%A9?q=100%25",
        "https://example.com/file?text=%FF",
        "report?token=notes",
        "//localhost/artmark-missing/file",
        "///tmp/artmark-file",
        "README.md",
    ]
    .map(str::to_owned)
    .to_vec();
    for nested in [
        "https://example.org/products?code=ABC&state=CA",
        "/products?id=1",
        "//example.org/products?id=1",
        "products?id=1",
    ] {
        for payload in [nested.to_owned(), encode(nested), encode(&encode(nested))] {
            cases.push(format!("https://example.com/file?next={payload}"));
            cases.push(format!("https://example.com/file#next={payload}"));
        }
    }
    cases
}
