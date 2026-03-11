BINARY     := murmur
INSTALL    := $(HOME)/.cargo/bin
CONFIG_DIR := $(HOME)/.config/murmur
SOUNDS_DIR := $(CONFIG_DIR)/sounds

.PHONY: install uninstall

install:
	cargo build --release
	install -m 755 target/release/$(BINARY) $(INSTALL)/$(BINARY)
	mkdir -p $(SOUNDS_DIR)
	-cp -n sounds/* $(SOUNDS_DIR)/
	@echo "Installed $(BINARY) to $(INSTALL)/$(BINARY)"
	@echo "Sounds    -> $(SOUNDS_DIR)"
	@echo "Presets   -> $(CONFIG_DIR)/presets.json"

uninstall:
	rm -f $(INSTALL)/$(BINARY)
	@echo "Removed $(INSTALL)/$(BINARY)"
	@echo "Config left intact at $(CONFIG_DIR)"
