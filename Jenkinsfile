pipeline {
    agent any

    environment {
        CARGO_TERM_COLOR = 'always'
        PATH = "${HOME}/.cargo/bin:${env.PATH}"
    }

    triggers {
        pollSCM('H/5 * * * *')
    }

    stages {
        stage('Setup') {
            steps {
                sh '''
                    if ! command -v cc > /dev/null 2>&1; then
                        sudo apt-get update && sudo apt-get install -y build-essential pkg-config
                    fi
                    if ! command -v rustup > /dev/null 2>&1; then
                        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
                        . "$HOME/.cargo/env"
                    fi
                    rustup component add clippy rustfmt
                    rustc --version
                    cargo --version
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
