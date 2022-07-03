FROM rust:1.61
# TODO: Specific commits for used repos, don't just use master HEAD

# Prevent being stuck at timezone selection
ENV TZ=Europe/London
RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone


RUN apt update && apt install -y gpg wget vim git g++ python3 python3-pip libclang-dev

# Build twint
RUN git clone --depth=1 https://github.com/minamotorin/twint.git && \
    cd twint && \
    pip3 install . -r requirements.txt

# Build websocat
RUN git clone https://github.com/vi/websocat.git && \
    cd websocat && \
    cargo install --features=ssl websocat

# Build nostril
RUN git clone https://github.com/bitcoin-core/secp256k1 && \
    cd secp256k1 && \
    ./autogen.sh && \
    ./configure --enable-module-ecdh --enable-module-schnorrsig && \
    make install
    # cd nostril && \
    # sed -i 's/\(^CFLAGS.*\)/\1 -fPIC/' Makefile && \
    # echo "libnostril.so: \$(OBJS)\n\tgcc -shared -o \$@ \$(OBJS)  -lsecp256k1" >> /nostril/Makefile && \
	 # echo 'int test() { return 123456; }' >> nostril.c && \
	 # echo 'int test();' >> nostril.h
	 # make libnostril.so

COPY . /app

# TODO: Add non-root user and use it
ENV LD_LIBRARY_PATH=/usr/local/lib:/nostril/
# RUN bash -c 'cd /app && cargo run --release'
RUN bash
