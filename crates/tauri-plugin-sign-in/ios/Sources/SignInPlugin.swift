import AuthenticationServices
import CryptoKit
import Foundation
import Security
import Tauri
import UIKit
import WebKit

struct GoogleArgs: Decodable {
  let clientId: String
  let iosClientId: String
  let scheme: String
  let nonce: String?
  let silent: Bool
}

struct AppleArgs: Decodable {
  let nonce: String
}

struct GoogleToken: Encodable {
  let idToken: String
}

struct AppleToken: Encodable {
  let idToken: String
  let authorizationCode: String
}

struct SignInError: Error {
  let code: String
  let message: String

  static func failed(_ message: String) -> SignInError {
    SignInError(code: "failed", message: message)
  }
}

private let googleAuthorize = URL(string: "https://accounts.google.com/o/oauth2/v2/auth")!
private let googleToken = URL(string: "https://oauth2.googleapis.com/token")!

/// Only RFC 3986's unreserved characters stay as they are. `URLComponents`
/// leaves `+` and `&` in a query, and Google reads a `+` as a space.
private func formEncode(_ pairs: [(String, String)]) -> String {
  let unreserved = CharacterSet(
    charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~")
  return pairs.map { key, value in
    let v = value.addingPercentEncoding(withAllowedCharacters: unreserved) ?? ""
    return "\(key)=\(v)"
  }.joined(separator: "&")
}

private func base64URL(_ data: Data) -> String {
  data.base64EncodedString()
    .replacingOccurrences(of: "+", with: "-")
    .replacingOccurrences(of: "/", with: "_")
    .replacingOccurrences(of: "=", with: "")
}

private func randomURLSafe(_ count: Int) -> String? {
  var bytes = [UInt8](repeating: 0, count: count)
  guard SecRandomCopyBytes(kSecRandomDefault, count, &bytes) == errSecSuccess else { return nil }
  return base64URL(Data(bytes))
}

private func sha256Hex(_ text: String) -> String {
  SHA256.hash(data: Data(text.utf8)).map { String(format: "%02x", $0) }.joined()
}

/// Google's refresh token, one Keychain item for each iOS client id. It never
/// leaves this device, and silent sign-in trades it for a fresh id token.
private struct RefreshTokens {
  let service = (Bundle.main.bundleIdentifier ?? "app") + ".sign-in.google"

  private func query(_ account: String) -> [String: Any] {
    [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: service,
      kSecAttrAccount as String: account,
      kSecAttrSynchronizable as String: kCFBooleanFalse as Any,
    ]
  }

  func read(_ account: String) -> String? {
    var ask = query(account)
    ask[kSecReturnData as String] = true
    ask[kSecMatchLimit as String] = kSecMatchLimitOne
    var found: CFTypeRef?
    guard SecItemCopyMatching(ask as CFDictionary, &found) == errSecSuccess,
      let data = found as? Data
    else { return nil }
    return String(data: data, encoding: .utf8)
  }

  func save(_ account: String, _ token: String) {
    let data = Data(token.utf8)
    let updated = SecItemUpdate(
      query(account) as CFDictionary, [kSecValueData as String: data] as CFDictionary)
    if updated == errSecItemNotFound {
      var add = query(account)
      add[kSecValueData as String] = data
      add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
      SecItemAdd(add as CFDictionary, nil)
    }
  }

  func delete(_ account: String) {
    SecItemDelete(query(account) as CFDictionary)
  }
}

/// Apple through `ASAuthorizationController`, Google through
/// `ASWebAuthenticationSession` with PKCE: the sign-in the GoogleSignIn SDK
/// runs, without the SDK.
class SignInPlugin: Plugin, ASAuthorizationControllerDelegate,
  ASAuthorizationControllerPresentationContextProviding,
  ASWebAuthenticationPresentationContextProviding
{
  private weak var webview: WKWebView?
  private let refreshTokens = RefreshTokens()

  // Each sheet holds its invoke until it answers. A second call while one is
  // open ends the first, because a sheet that never answered would block
  // signing in until a restart.
  private var appleOpen: (invoke: Invoke, controller: ASAuthorizationController)?
  private var googleOpen: ASWebAuthenticationSession?

  override func load(webview: WKWebView) {
    self.webview = webview
  }

  private func reject(_ invoke: Invoke, _ error: SignInError) {
    invoke.reject(error.message, code: error.code)
  }

  private var window: ASPresentationAnchor {
    if let window = webview?.window {
      return window
    }
    let scene =
      UIApplication.shared.connectedScenes
      .first { $0.activationState == .foregroundActive } as? UIWindowScene
    return scene?.windows.first { $0.isKeyWindow } ?? ASPresentationAnchor()
  }

  func presentationAnchor(for controller: ASAuthorizationController) -> ASPresentationAnchor {
    window
  }

  func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
    window
  }

  // MARK: Apple

  @objc public func apple(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(AppleArgs.self) else {
      reject(invoke, .failed("apple needs a nonce"))
      return
    }
    DispatchQueue.main.async {
      if let open = self.appleOpen {
        self.appleOpen = nil
        self.reject(open.invoke, .failed("Another Apple sign-in started"))
      }
      let request = ASAuthorizationAppleIDProvider().createRequest()
      // Email only: the account keeps no name.
      request.requestedScopes = [.email]
      request.nonce = sha256Hex(args.nonce)
      let controller = ASAuthorizationController(authorizationRequests: [request])
      controller.delegate = self
      controller.presentationContextProvider = self
      self.appleOpen = (invoke, controller)
      controller.performRequests()
    }
  }

  private func takeApple(_ controller: ASAuthorizationController) -> Invoke? {
    guard let open = appleOpen, open.controller === controller else { return nil }
    appleOpen = nil
    return open.invoke
  }

  func authorizationController(
    controller: ASAuthorizationController,
    didCompleteWithAuthorization authorization: ASAuthorization
  ) {
    guard let invoke = takeApple(controller) else { return }
    guard let credential = authorization.credential as? ASAuthorizationAppleIDCredential,
      let token = credential.identityToken.flatMap({ String(data: $0, encoding: .utf8) }),
      let code = credential.authorizationCode.flatMap({ String(data: $0, encoding: .utf8) })
    else {
      reject(invoke, .failed("Apple answered no identity token or authorization code"))
      return
    }
    invoke.resolve(AppleToken(idToken: token, authorizationCode: code))
  }

  func authorizationController(
    controller: ASAuthorizationController, didCompleteWithError error: Error
  ) {
    guard let invoke = takeApple(controller) else { return }
    let nsError = error as NSError
    switch (error as? ASAuthorizationError)?.code {
    case .canceled:
      reject(invoke, SignInError(code: "cancelled", message: "The player closed the Apple sheet"))
    case .unknown:
      // 1000 comes both from a build without the Sign in with Apple
      // entitlement and from a player who closes "sign in to your Apple
      // Account" on a phone with none.
      reject(
        invoke,
        .failed(
          "Apple answered 1000: the phone has no Apple Account signed in, or the app lacks the Sign in with Apple capability"
        ))
    default:
      reject(invoke, .failed("Apple answered \(nsError.code): \(error.localizedDescription)"))
    }
  }

  // MARK: Google

  @objc public func google(_ invoke: Invoke) {
    guard let args = try? invoke.parseArgs(GoogleArgs.self) else {
      reject(invoke, .failed("google needs clientId, iosClientId and scheme"))
      return
    }
    if args.silent {
      refresh(args, invoke)
    } else {
      DispatchQueue.main.async { self.browse(args, invoke) }
    }
  }

  /// A silent sign-in trades the stored refresh token for a fresh id token.
  /// Google puts no nonce in a refreshed token.
  private func refresh(_ args: GoogleArgs, _ invoke: Invoke) {
    guard let token = refreshTokens.read(args.iosClientId) else {
      reject(
        invoke,
        SignInError(code: "no-account", message: "No Google account signed in on this phone yet"))
      return
    }
    post([
      ("grant_type", "refresh_token"),
      ("client_id", args.iosClientId),
      ("refresh_token", token),
      ("audience", args.clientId),
    ]) { result in
      switch result {
      case .success(let body):
        guard let idToken = body["id_token"] as? String else {
          self.reject(invoke, .failed("Google answered no id token"))
          return
        }
        invoke.resolve(GoogleToken(idToken: idToken))
      case .failure(let error) where error.code == "invalid_grant":
        // The player revoked the app, or Google expired the token.
        self.refreshTokens.delete(args.iosClientId)
        self.reject(invoke, SignInError(code: "no-account", message: error.message))
      case .failure(let error):
        self.reject(invoke, error)
      }
    }
  }

  private func browse(_ args: GoogleArgs, _ invoke: Invoke) {
    guard let verifier = randomURLSafe(32), let state = randomURLSafe(16) else {
      reject(invoke, .failed("The phone gave no random bytes"))
      return
    }
    let challenge = base64URL(Data(SHA256.hash(data: Data(verifier.utf8))))
    let redirect = args.scheme + ":/oauth2callback"
    var query = [
      ("client_id", args.iosClientId),
      ("redirect_uri", redirect),
      ("response_type", "code"),
      ("scope", "openid email"),
      ("code_challenge", challenge),
      ("code_challenge_method", "S256"),
      ("state", state),
      ("audience", args.clientId),
    ]
    if let nonce = args.nonce {
      query.append(("nonce", nonce))
    }
    guard let url = URL(string: googleAuthorize.absoluteString + "?" + formEncode(query)) else {
      reject(invoke, .failed("The Google sign-in address did not build"))
      return
    }

    googleOpen?.cancel()
    var session: ASWebAuthenticationSession?
    session = ASWebAuthenticationSession(url: url, callbackURLScheme: args.scheme) {
      callback, error in
      if self.googleOpen === session {
        self.googleOpen = nil
      }
      if let error {
        if (error as? ASWebAuthenticationSessionError)?.code == .canceledLogin {
          self.reject(
            invoke, SignInError(code: "cancelled", message: "The player closed the Google sheet"))
        } else {
          self.reject(invoke, .failed("The Google sheet failed: \(error.localizedDescription)"))
        }
        return
      }
      let items =
        callback.flatMap { URLComponents(url: $0, resolvingAgainstBaseURL: false) }?.queryItems
        ?? []
      let answer = Dictionary(
        items.map { ($0.name, $0.value ?? "") }, uniquingKeysWith: { first, _ in first })
      guard answer["state"] == state else {
        self.reject(invoke, .failed("Google answered for another sign-in"))
        return
      }
      if let error = answer["error"] {
        let code = error == "access_denied" ? "cancelled" : "failed"
        self.reject(invoke, SignInError(code: code, message: "Google answered \(error)"))
        return
      }
      guard let code = answer["code"], !code.isEmpty else {
        self.reject(invoke, .failed("Google answered no code"))
        return
      }
      self.exchange(args, code: code, verifier: verifier, redirect: redirect, invoke)
    }
    guard let session else { return }
    session.presentationContextProvider = self
    // A shared session keeps the player's Google sign-in in Safari, so the
    // sheet offers their accounts instead of an empty form.
    session.prefersEphemeralWebBrowserSession = false
    googleOpen = session
    if !session.start() {
      googleOpen = nil
      reject(invoke, .failed("The Google sheet did not open"))
    }
  }

  private func exchange(
    _ args: GoogleArgs, code: String, verifier: String, redirect: String, _ invoke: Invoke
  ) {
    post([
      ("grant_type", "authorization_code"),
      ("code", code),
      ("client_id", args.iosClientId),
      ("redirect_uri", redirect),
      ("code_verifier", verifier),
      ("audience", args.clientId),
    ]) { result in
      switch result {
      case .success(let body):
        guard let idToken = body["id_token"] as? String else {
          self.reject(invoke, .failed("Google answered no id token"))
          return
        }
        if let refresh = body["refresh_token"] as? String {
          self.refreshTokens.save(args.iosClientId, refresh)
        }
        invoke.resolve(GoogleToken(idToken: idToken))
      case .failure(let error):
        self.reject(invoke, error)
      }
    }
  }

  private func post(
    _ form: [(String, String)], then: @escaping (Result<[String: Any], SignInError>) -> Void
  ) {
    var request = URLRequest(url: googleToken)
    request.httpMethod = "POST"
    request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
    request.httpBody = Data(formEncode(form).utf8)
    URLSession.shared.dataTask(with: request) { data, response, error in
      if let error {
        then(.failure(.failed("Google could not be reached: \(error.localizedDescription)")))
        return
      }
      let status = (response as? HTTPURLResponse)?.statusCode ?? 0
      let body =
        data.flatMap { try? JSONSerialization.jsonObject(with: $0) } as? [String: Any] ?? [:]
      if status == 200 {
        then(.success(body))
        return
      }
      let code = body["error"] as? String ?? "http \(status)"
      let detail = body["error_description"] as? String ?? ""
      then(.failure(SignInError(code: code, message: "Google answered \(code) \(detail)")))
    }.resume()
  }
}

@_cdecl("init_plugin_sign_in")
func initPlugin() -> Plugin {
  return SignInPlugin()
}
