package dev.hoangmirs.signin

import android.app.Activity
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
}
