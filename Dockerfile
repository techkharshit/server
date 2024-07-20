# Use an official Rust image as a base with the latest version
FROM rust:latest as builder

# Set the working directory
WORKDIR /usr/src/app

# Copy the Cargo files first to leverage Docker cache
COPY Cargo.toml Cargo.lock ./

# Copy the source code
COPY . .

# Build the application
RUN cargo build --release

# Use the latest Debian image with GLIBC 2.36
FROM debian:bookworm-slim

# Install required libraries
RUN apt-get update && apt-get install -y \
    libssl-dev \
    ca-certificates \
    libmariadb-dev \
    netcat-openbsd \
    && rm -rf /var/lib/apt/lists/*

# Copy the compiled binary from the builder stage
COPY --from=builder /usr/src/app/target/release/server /usr/local/bin/server

# Copy the wait-for-it script
COPY wait-for-it.sh /usr/local/bin/wait-for-it.sh
RUN chmod +x /usr/local/bin/wait-for-it.sh

# Copy the .env file
COPY .env /usr/src/app/.env

# Expose the port
EXPOSE 8000

# Run the application
CMD ["/usr/local/bin/server"]
