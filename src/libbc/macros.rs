#[macro_export]
macro_rules! round {
    ($x:expr, $scale:expr) => {
        ($x * $scale).round() / $scale
    };
}
#[macro_export]
macro_rules! ceil {
    ($x:expr, $scale:expr) => {
        ($x * $scale).ceil() / $scale
    };
}
#[macro_export]
macro_rules! floor {
    ($x:expr, $scale:expr) => {
        ($x * $scale).floor() / $scale
    };
}
#[macro_export]
macro_rules! concat_str {
    ($x:expr) => {
        $x
    };
}

#[macro_export]
macro_rules! vec_of_strings {
    ($($x:expr),*) => (vec![$($x.to_string()),*]);
}

#[macro_export]
macro_rules! format_duration {
    ($($x:expr), *) =>(format!("{:02}:{:02}", $($x / 60, $x % 60),*));
}

#[macro_export]
macro_rules! measure {
    ( $x:expr) => {{
        let start = std::time::Instant::now();
        let result = $x;
        let end = start.elapsed();
        println!("{}.{:03} sec elapsed.", end.as_secs(), end.subsec_millis());
        result
    }};
}

#[macro_export]
macro_rules! hashmap {
    ($( $key: expr => $val: expr ),*) => {{
        let mut map = ::std::collections::HashMap::new();
        $(map.insert($key, $val);)*
        map
    }}
}

#[macro_export]
macro_rules! lazy_regex {
    ($($x:ident:$y:tt),*) => {
        $(pub static $x : LazyLock<Regex> = LazyLock::new(|| Regex::new($y).unwrap());)*};
}
