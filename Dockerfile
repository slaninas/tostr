FROM rust:1.61
# TODO: Specific commits for used repos, don't just use master HEAD

# Prevent being stuck at timezone selection
ENV TZ=Europe/London
RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone


RUN apt update && apt install -y gpg wget vim git g++ python3 python3-pip

# Build twint
RUN git clone --depth=1 https://github.com/minamotorin/twint.git && \
    cd twint && \
    pip3 install . -r requirements.txt

COPY . /app

# TODO: Add non-root user and use it
ENV RUST_LOG=info
RUN bash -c 'cd /app && cargo run --release'
