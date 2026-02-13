pipeline {
    agent any

    environment {
        CARGO_TERM_COLOR = 'always'
        PYO3_NO_PYTHON = '1'
        PATH = "${HOME}/.local/bin:${HOME}/.cargo/bin:${env.PATH}"
        LD_LIBRARY_PATH = "${HOME}/.gcc-root/usr/lib/aarch64-linux-gnu:${HOME}/.gcc-root/usr/lib"
    }

    triggers {
        pollSCM('H/5 * * * *')
    }

    stages {
        stage('Setup') {
            steps {
                sh '''
                    # Install GCC from Debian packages (no root required)
                    if ! [ -f "$HOME/.gcc-root/.setup-done" ]; then
                        rm -rf "$HOME/.gcc-root"
                        echo "Installing GCC toolchain from Debian packages..."
                        MIRROR="http://deb.debian.org/debian"
                        TMP=$(mktemp -d)

                        curl -sL "$MIRROR/dists/bookworm/main/binary-arm64/Packages.gz" | \
                            gzip -d > "$TMP/Packages"

                        for pkg in gcc-12 cpp-12 binutils-aarch64-linux-gnu binutils-common \
                                   libbinutils libctf-nobfd0 libctf0 libgprofng0 libjansson4 \
                                   libc6-dev linux-libc-dev libgcc-12-dev libcc1-0; do
                            FILE=$(sed -n "/^Package: ${pkg}$/,/^$/s/^Filename: //p" "$TMP/Packages" | head -1)
                            if [ -n "$FILE" ]; then
                                curl -sL "$MIRROR/$FILE" -o "$TMP/pkg.deb"
                                dpkg-deb -x "$TMP/pkg.deb" "$HOME/.gcc-root"
                            fi
                        done

                        rm -rf "$TMP"

                        # Fix absolute paths in linker scripts
                        for f in "$HOME/.gcc-root/usr/lib/aarch64-linux-gnu/"*.so; do
                            if [ -f "$f" ] && grep -q "GNU ld script" "$f" 2>/dev/null; then
                                sed -i "s|/usr/lib/aarch64-linux-gnu|$HOME/.gcc-root/usr/lib/aarch64-linux-gnu|g" "$f"
                            fi
                        done
                        touch "$HOME/.gcc-root/.setup-done"
                    fi

                    mkdir -p "$HOME/.local/bin"
                    ln -sf "$HOME/.gcc-root/usr/bin/aarch64-linux-gnu-gcc-12" "$HOME/.local/bin/cc"
                    ln -sf "$HOME/.gcc-root/usr/bin/aarch64-linux-gnu-gcc-12" "$HOME/.local/bin/gcc"
                    ln -sf "$HOME/.gcc-root/usr/bin/aarch64-linux-gnu-ar" "$HOME/.local/bin/ar"
                    ln -sf "$HOME/.gcc-root/usr/bin/aarch64-linux-gnu-ld" "$HOME/.local/bin/ld"
                    ln -sf "$HOME/.gcc-root/usr/bin/aarch64-linux-gnu-as" "$HOME/.local/bin/as"

                    if ! command -v rustup > /dev/null 2>&1; then
                        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
                        . "$HOME/.cargo/env"
                    fi
                    rustup component add clippy rustfmt
                    rustc --version
                    cargo --version
                    cc --version
                '''
            }
        }

        stage('Quality Checks') {
            parallel {
                stage('Format') {
                    steps {
                        sh 'cargo fmt --all -- --check'
                    }
                }
                stage('Clippy') {
                    steps {
                        sh 'cargo clippy --workspace --exclude hwpx-python --all-targets --all-features -- -D warnings'
                    }
                }
            }
        }

        stage('Test') {
            steps {
                sh 'cargo test --workspace --exclude hwpx-python'
            }
        }

        stage('Security Audit') {
            steps {
                sh '''
                    cargo install cargo-audit --quiet || true
                    cargo audit
                '''
            }
        }
    }

    post {
        always {
            cleanWs()
        }
        failure {
            echo "CI pipeline failed on branch: ${env.GIT_BRANCH}"
        }
        success {
            echo "CI pipeline passed on branch: ${env.GIT_BRANCH}"
        }
    }
}
