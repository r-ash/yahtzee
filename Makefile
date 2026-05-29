.PHONY: wasm

wasm:
	wasm-pack build yahtzee-wasm --target web --release
