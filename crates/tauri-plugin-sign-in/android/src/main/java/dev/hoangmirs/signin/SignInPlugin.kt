package dev.hoangmirs.signin

import android.app.Activity
import android.net.Uri
import androidx.activity.ComponentActivity
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import androidx.browser.auth.AuthTabIntent
import android.os.CancellationSignal
import androidx.credentials.CredentialManager
import androidx.credentials.CredentialManagerCallback
import androidx.credentials.CustomCredential
import androidx.credentials.GetCredentialRequest
import androidx.credentials.GetCredentialResponse
import androidx.credentials.exceptions.GetCredentialCancellationException
import androidx.credentials.exceptions.GetCredentialException
import androidx.credentials.exceptions.GetCredentialProviderConfigurationException
import androidx.credentials.exceptions.GetCredentialUnsupportedException
import androidx.credentials.exceptions.NoCredentialException
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.android.libraries.identity.googleid.GetGoogleIdOption
import com.google.android.libraries.identity.googleid.GoogleIdTokenCredential
import com.google.android.libraries.identity.googleid.GoogleIdTokenParsingException
import java.util.concurrent.Executor

@InvokeArg
class WebArgs {
  var url: String = ""
  var scheme: String = ""
}

@InvokeArg
class GoogleArgs {
  var clientId: String = ""
  var nonce: String? = null
  var silent: Boolean = false
}

/** Google through Credential Manager's own sheet. Apple has no sheet on
 * Android, and the Rust side answers `unavailable` for it. */
@TauriPlugin
class SignInPlugin(private val activity: Activity) : Plugin(activity) {
  private val credentials = CredentialManager.create(activity)

  // A second call while a sheet is open ends the first, because a sheet that
  // never answered would block signing in until a restart.
  private var open: CancellationSignal? = null

  private fun reject(invoke: Invoke, code: String, message: String?) =
    invoke.reject(message?.ifEmpty { null } ?: "The Google sign-in failed", code)

  @Command
  fun google(invoke: Invoke) {
    val args = invoke.parseArgs(GoogleArgs::class.java)
    // Silent offers only accounts that chose this app before, and signs in
    // with no tap when there is one.
    val option = GetGoogleIdOption.Builder()
      .setServerClientId(args.clientId)
      .setFilterByAuthorizedAccounts(args.silent)
      .setAutoSelectEnabled(args.silent)
      .apply { args.nonce?.let { setNonce(it) } }
      .build()
    val request = GetCredentialRequest.Builder().addCredentialOption(option).build()
    open?.cancel()
    val signal = CancellationSignal()
    open = signal
    credentials.getCredentialAsync(
      activity,
      request,
      signal,
      Executor(Runnable::run),
      object : CredentialManagerCallback<GetCredentialResponse, GetCredentialException> {
        override fun onResult(result: GetCredentialResponse) {
          if (open === signal) open = null
          val credential = result.credential
          if (credential !is CustomCredential ||
            credential.type != GoogleIdTokenCredential.TYPE_GOOGLE_ID_TOKEN_CREDENTIAL
          ) {
            return reject(invoke, "failed", "Credential Manager answered a ${credential.type}")
          }
          try {
            val token = GoogleIdTokenCredential.createFrom(credential.data).idToken
            invoke.resolve(JSObject().put("idToken", token))
          } catch (e: GoogleIdTokenParsingException) {
            reject(invoke, "failed", e.message)
          }
        }

        override fun onError(e: GetCredentialException) {
          if (open === signal) open = null
          val code = when (e) {
            is GetCredentialCancellationException -> "cancelled"
            is NoCredentialException -> "no-account"
            is GetCredentialProviderConfigurationException,
            is GetCredentialUnsupportedException -> "unavailable"
            else -> "failed"
          }
          reject(invoke, code, e.errorMessage?.toString() ?: e.type)
        }
      },
    )
  }

  // Spike: a sign-in on the provider's web page, in an Auth Tab that hands
  // back the URL it ends on. Chrome before 137 opens a Custom Tab instead, and
  // the result is then cancelled: the app must catch the link itself.
  private var webOpen: Invoke? = null
  private var webLauncher: ActivityResultLauncher<android.content.Intent>? = null

  @Command
  fun web(invoke: Invoke) {
    val args = invoke.parseArgs(WebArgs::class.java)
    val owner = activity as? ComponentActivity
      ?: return reject(invoke, "unavailable", "The activity cannot start an Auth Tab")
    activity.runOnUiThread {
      webOpen?.let { reject(it, "cancelled", "Another web sign-in started") }
      webOpen = invoke
      // The registry, not registerForActivityResult: a plugin loads after its
      // activity started, and a lifecycle owner then refuses to register.
      val launcher = webLauncher ?: owner.activityResultRegistry.register(
        "sign-in-web", ActivityResultContracts.StartActivityForResult()
      ) { result ->
        val open = webOpen ?: return@register
        webOpen = null
        val back = result.data?.data
        if (result.resultCode == AuthTabIntent.RESULT_OK && back != null) {
          open.resolve(JSObject().put("url", back.toString()))
        } else {
          reject(open, "cancelled", "The web sign-in ended with ${result.resultCode}")
        }
      }.also { webLauncher = it }
      AuthTabIntent.Builder().build().launch(launcher, Uri.parse(args.url), args.scheme)
    }
  }
}
