use inquire::Select;
#[macro_export]
macro_rules! vec_of_strings {
    ($($x:expr),*) => (vec![$($x.to_string()),*]);
}
fn main() {
    let options = vec_of_strings!(
        "Banana",
        "Apple",
        "Strawberry",
        "Grapes",
        "Lemon",
        "Tangerine",
        "Watermelon",
        "Orange",
        "Pear",
        "Avocado",
        "Pineapple"
    );

    let ans = Select::new("What's your favorite fruit?", options)
        .with_raw_return(true)
        .prompt();

    match ans {
        Ok(choice) => println!("{choice}! That's mine too!"),
        Err(_) => println!("There was an error, please try again"),
    }
}
