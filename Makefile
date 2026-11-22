SHELL := /bin/bash
.DEFAULT_GOAL := help

CORE_MANIFEST := core/Cargo.toml
APP_DIR := app

.PHONY: help core-test core-clippy core-fmt ocr-test frontend-install frontend-build tauri-dev tauri-build models hero

help:
	@printf "Zeidex developer tasks\n\n"
	@printf "Rust core\n"
	@printf "  make core-test       - cargo test the core workspace\n"
	@printf "  make core-clippy     - clippy core libs, denying unwrap/expect\n"
	@printf "  make core-fmt        - cargo fmt --check\n"
	@printf "  make ocr-test        - real models: OCR, table grid, accuracy, PDF, Word/Excel export\n\n"
	@printf "OCR models (one-time, network)\n"
	@printf "  make models          - download the ONNX models into third_party/models\n\n"
	@printf "Desktop shell\n"
	@printf "  make frontend-install - npm install in app/\n"
	@printf "  make frontend-build  - typecheck + build the React app\n"
	@printf "  make tauri-dev       - run the app in development mode\n"
	@printf "  make tauri-build     - build the release bundle\n"
	@printf "  make deb             - build the .deb and repair its GTK dependency\n\n"
	@printf "Assets\n"
	@printf "  make hero            - re-render the animated README banner\n"

core-test:
	cargo test --workspace --manifest-path $(CORE_MANIFEST)

core-clippy:
	cargo clippy --workspace --lib --manifest-path $(CORE_MANIFEST) -- -D clippy::unwrap_used -D clippy::expect_used

core-fmt:
	cargo fmt --all --manifest-path $(CORE_MANIFEST) -- --check

frontend-install:
	cd $(APP_DIR) && npm install

frontend-build:
	cd $(APP_DIR) && npm run build

tauri-dev:
	./app/node_modules/.bin/tauri dev

tauri-build:
	./app/node_modules/.bin/tauri build

# tauri-cli appends `libgtk-3-0` after the configured dependencies, and that
# name does not exist on Ubuntu 24.04+/Debian 13 (it is `libgtk-3-0t64` there).
# The repair script rewrites the control file so the package installs on both
# naming schemes; see its header for the full reason.
deb:
	./app/node_modules/.bin/tauri build --bundles deb
	./scripts/fix-deb-depends.sh src-tauri/target/release/bundle/deb/*.deb

ocr-test:
	@test -f third_party/models/ppocrv4_det.onnx || \
		{ echo "run 'make models' first" >&2; exit 1; }
	@export ZEIDEX_TEST_MODELS_DIR="$(CURDIR)/third_party/models"; \
	 export ZEIDEX_TEST_IMAGE="$(CURDIR)/core/ocr_engine/tests/fixtures/table.png"; \
	 export ZEIDEX_TEST_ARABIC_IMAGE="$(CURDIR)/core/ocr_engine/tests/fixtures/arabic_lines.png"; \
	 export ZEIDEX_TEST_TABLE_IMAGE="$(CURDIR)/core/ocr_engine/tests/fixtures/table.png"; \
	 export ZEIDEX_TEST_PDF="$(CURDIR)/core/ocr_engine/tests/fixtures/table.pdf"; \
	 export ZEIDEX_TEST_PHOTO="$(CURDIR)/core/ocr_engine/tests/fixtures/photo-table.jpg"; \
	 if [ -f "$(CURDIR)/src-tauri/resources/lib/libpdfium.so" ]; then \
		export ZEIDEX_TEST_PDFIUM_LIB="$(CURDIR)/src-tauri/resources/lib/libpdfium.so"; \
	 fi; \
	 if [ -n "$$ZEIDEX_ORT_DYLIB" ]; then export ORT_DYLIB_PATH="$$ZEIDEX_ORT_DYLIB"; fi; \
	 cargo test --manifest-path $(CORE_MANIFEST) -p zeidex-ocr \
		--test ocr_smoke --test layout_smoke --test table_smoke --test ocr_accuracy \
		--test arabic_smoke --test pdf_smoke --test photo_smoke -- --nocapture && \
	 cargo test --manifest-path $(CORE_MANIFEST) -p zeidex-exporter \
		--test export ocr_output_exports_to_word_and_excel -- --nocapture

models:
	./scripts/fetch_models.sh

hero:
	python3 tools/render-hero.py
