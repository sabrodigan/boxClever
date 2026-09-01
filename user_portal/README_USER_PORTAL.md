# User Portal - Ruby on Rails Application

A simple, secure Ruby on Rails application for user registration, authentication, and profile management.

## Features

- **User Registration**: Create new accounts with email and password
- **User Authentication**: Secure login/logout functionality
- **Profile Management**: View and edit user information including:
  - First Name
  - Last Name
  - Email
  - Phone Number
  - Address
- **Password Management**: Change password from profile settings
- **Responsive Design**: Modern, mobile-friendly UI

## Requirements

- Ruby 3.2.3 or higher
- Rails 8.1.3.1 or higher
- SQLite3
- Bundler

## Installation

1. Navigate to the application directory:
```bash
cd user_portal
```

2. Install dependencies:
```bash
bundle install
```

3. Set up the database:
```bash
bin/rails db:migrate
```

## Running the Application

Start the Rails server:
```bash
bin/rails server
```

The application will be available at `http://localhost:3000`

## Usage

### Creating an Account

1. Visit the home page
2. Click "Sign Up" in the navigation bar
3. Fill in the registration form with:
   - First Name (required)
   - Last Name (required)
   - Email (required, must be valid email format)
   - Phone (optional)
   - Address (optional)
   - Password (required, minimum 6 characters)
   - Password Confirmation (required)
4. Click "Create Account"

### Logging In

1. Click "Login" in the navigation bar
2. Enter your email and password
3. Click "Login"

### Viewing Profile

After logging in, your profile information is displayed on the home page.

### Editing Profile

1. Click "Edit Profile" in the navigation bar
2. Modify any of your information
3. Optionally change your password by filling in the password fields
4. Click "Update Profile"

### Logging Out

Click "Logout" in the navigation bar to end your session.

## Security Features

- Passwords are securely hashed using bcrypt
- CSRF protection enabled
- Email validation and uniqueness enforcement
- Secure session management
- Password minimum length requirement

## Development

### Database Schema

The User model includes the following fields:
- `email` (string, required, unique, indexed)
- `password_digest` (string, required)
- `first_name` (string, required)
- `last_name` (string, required)
- `phone` (string, optional)
- `address` (text, optional)

### Routes

- `GET /` - Home page
- `GET /signup` - Registration page
- `POST /signup` - Create user
- `GET /login` - Login page
- `POST /login` - Authenticate user
- `DELETE /logout` - Logout
- `GET /profile/edit` - Edit profile page
- `PATCH /profile` - Update profile

## Testing

Run the Rails console to interact with the application:
```bash
bin/rails console
```

Example commands:
```ruby
# Create a user
user = User.create(
  first_name: "John",
  last_name: "Doe",
  email: "john@example.com",
  password: "password123",
  password_confirmation: "password123"
)

# Find a user
user = User.find_by(email: "john@example.com")

# Authenticate a user
user.authenticate("password123")
```

## License

This is a demonstration application created for learning purposes.
