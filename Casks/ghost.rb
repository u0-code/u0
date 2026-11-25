# Ghost's newest release. .github/workflows/app.yml sets the version and the
# sha256 when it publishes one.
cask "ghost" do
  version "0.1.0-pre.1"
  sha256 "a39e72999ea22918b749c8cdbee8b82b9ce9dbb17b591db76c3989ae97824dfd"

  url "https://github.com/u0-code/u0/releases/download/v#{version}/Ghost-#{version}-macos-arm64.zip"
  name "Ghost"
  desc "Desktop app for the coding agents you already use"
  homepage "https://github.com/u0-code/u0"

  conflicts_with cask: "ghost@nightly"
  depends_on arch: :arm64
  depends_on macos: :ventura

  app "Ghost.app"

  # Ghost is signed ad hoc, not notarized, and Gatekeeper refuses to open
  # it while it is quarantined: installing this cask is the choice to open it.
  postflight_steps do
    run "/usr/bin/xattr", args: ["-dr", "com.apple.quarantine", "{{appdir}}/Ghost.app"],
                          writable_paths: ["Ghost.app"], writable_base: :appdir
  end

  zap trash: "~/.ghost"

  caveats "Ghost is not notarized; this cask removed the quarantine from Ghost.app so that it opens."
end
