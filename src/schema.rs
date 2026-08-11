diesel::table! {
    users (discord_id) {
        discord_id -> Text,
        display_name -> Text,
        avatar_hash -> Nullable<Text>,
        created_at -> BigInt,
    }
}

diesel::table! {
    sessions (token_hash) {
        token_hash -> Text,
        user_id -> Text,
        created_at -> BigInt,
        expires_at -> BigInt,
    }
}

diesel::table! {
    quotes (id) {
        id -> Text,
        user_id -> Text,
        title -> Text,
        description -> Text,
        is_visible -> Bool,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    quote_sections (id) {
        id -> Integer,
        quote_id -> Text,
        position -> Integer,
        title -> Text,
        description -> Text,
        estimate_min_minutes -> Integer,
        estimate_max_minutes -> Nullable<Integer>,
        price_cents -> BigInt,
    }
}

diesel::joinable!(sessions -> users (user_id));
diesel::joinable!(quotes -> users (user_id));
diesel::joinable!(quote_sections -> quotes (quote_id));
diesel::allow_tables_to_appear_in_same_query!(quote_sections, quotes, sessions, users);
