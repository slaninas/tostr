int test() {
	return 9876543210;
}

void nostril_get_keys(unsigned char * secret, unsigned char * pub) {
	secp256k1_context * ctx;
	struct key key;

	if (!init_secp_context(&ctx)) {
		return 2;
	}

	if (!generate_key(ctx, &key)) {
		fprintf(stderr, "counld not generate key\n");
		return 4;
	}
	fprintf(stderr, "secret_key ");
	print_hex(key.secret, sizeof(key.secret));

	memcpy(secret, key.secret, sizeof(key.secret));
	memcpy(pub, key.pubkey, sizeof(key.pubkey));

}

void nostril_create_event(unsigned char * pubkey, unsigned char * secret, char * content,  unsigned char * created_event) {
	printf("content >%s<\n", content);

	secp256k1_context * ctx;
	if (!init_secp_context(&ctx)) {
		return 2;
	}

	struct key key;

	if (!decode_key(ctx, secret, &key)) {
		fprintf(stderr, "could not decode key\n");
		return 8;
	}

	struct nostr_event ev = {0};
	ev.created_at = time(NULL);
	ev.content = content;
	ev.kind = 1;

	memcpy(ev.pubkey, pubkey, 32);

	generate_event_id(&ev);

	if (!sign_event(ctx, &key, &ev)) {
		fprintf(stderr, "could not sign event\n");
		return 6;
	}

	if (!print_event(&ev, 1)) {
		fprintf(stderr, "buffer too small\n");
		return 88;
	}

}
