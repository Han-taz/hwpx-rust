pipeline {
    agent any

    environment {
        CARGO_TERM_COLOR = 'always'
        RUSTUP_HOME = "${WORKSPACE}/.rustup"
        CARGO_HOME = "${WORKSPACE}/.cargo"
        PATH = "${WORKSPACE}/.cargo/bin:${env.PATH}"
    }

    triggers {
        pollSCM('H/5 * * * *')
    }

    stages {
        stage('Checkout') {
            steps {
                checkout scm
            }
        }

        stage('Setup Rust') {
            steps {
                sh '''
                    if ! command -v rustup &> /dev/null; then
                        curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
                    fi
                    rustup update stable
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
            echo "CI pipeline failed on branch: ${env.BRANCH_NAME}"
        }
        success {
            echo "CI pipeline passed on branch: ${env.BRANCH_NAME}"
        }
    }
}
