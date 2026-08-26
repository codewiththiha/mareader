pub const WORD_INFO_SYSTEM_PROMPT: &str = r#"
You are an expert linguist and dictionary assistant.
Analyze the selected word based on the provided context.
Return ONLY valid JSON matching the requested schema. Do not include markdown formatting or explanations outside the JSON.
"#;

pub const WORD_INFO_USER_PROMPT: &str = r#"
Analyze the following word in the context of the sentence provided.

Word: "{word}"
Context: "{context}"
"#;
