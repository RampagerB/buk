# Homebrew formula for buk.
#
# Publish by putting this file in a repository named `homebrew-buk`
# (e.g. RampagerB/homebrew-buk) at Formula/buk.rb. Users then install with:
#
#   brew tap RampagerB/buk && brew install buk
#
# Update `url` + `sha256` per release — the sha256 is of the source tarball:
#   curl -sSL https://github.com/RampagerB/buk/archive/refs/tags/v<X.Y.Z>.tar.gz | sha256sum
class Buk < Formula
  desc "Back up files and directories to a dated, mirrored backup root"
  homepage "https://github.com/RampagerB/buk"
  url "https://github.com/RampagerB/buk/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "6e8d0240790ec7981fc566a2cfa8d19b862ddb6c97522059bd89f098f12d053b"
  license "Apache-2.0"
  head "https://github.com/RampagerB/buk.git", branch: "main"

  depends_on "rust" => :build

  def install
    # std_cargo_args already passes --jobs, --locked, --root and --path.
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "buk #{version}", shell_output("#{bin}/buk --help")
  end
end
