pipeline {
    agent any

    environment {
        CARGO_TERM_COLOR = 'always'
        PATH = "${HOME}/.local/bin:${HOME}/.cargo/bin:${env.PATH}"
    }

    triggers {
        pollSCM('H/5 * * * *')
    }

    stages {
        stage('Setup') {
            steps {
                sh '''
                    if ! command -v cc > /dev/null 2>&1; then
                        if ! [ -d "$HOME/aarch64-linux-musl-native" ]; then
                            echo "Installing C toolchain (musl-gcc)..."
                            curl -sL https://musl.cc/aarch64-linux-musl-native.tgz | tar xz -C "$HOME"
                        fi
                        mkdir -p "$HOME/.local/bin"
                        ln -sf "$HOME/aarch64-linux-musl-native/bin/aarch64-linux-musl-gcc" "$HOME/.local/bin/cc"
                        ln -sf "$HOME/aarch64-linux-musl-native/bin/aarch64-linux-musl-gcc" "$HOME/.local/bin/gcc"
                        ln -sf "$HOME/aarch64-linux-musl-native/bin/aarch64-linux-musl-ar" "$HOME/.local/bin/ar"
                    fi
                    if ! command -v rustup > /dev/null 2>&1; then
                        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-host aarch64-unknown-linux-musl
                        . "$HOME/.cargo/env"
                    fi
                    rustup toolchain install stable-aarch64-unknown-linux-musl
                    rustup default stable-aarch64-unknown-linux-musl
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
                        sh 'cargo clippy --workspace --all-targets --all-features -- -D warnings'
                    }
                }
            }
        }

        stage('Test') {
            steps {
                sh 'cargo test --workspace'
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
