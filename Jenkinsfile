pipeline {
    agent {
        docker {
            image 'rust:1.93'
            args '-v jenkins-cargo-cache:/usr/local/cargo/registry'
        }
    }

    environment {
        CARGO_TERM_COLOR = 'always'
    }

    triggers {
        pollSCM('H/5 * * * *')
    }

    stages {
        stage('Setup') {
            steps {
                sh '''
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
