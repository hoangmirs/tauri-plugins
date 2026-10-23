package dev.hoangmirs.vault

import android.app.Activity
import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyInfo
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import android.util.AtomicFile
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.FileNotFoundException
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction
import java.security.GeneralSecurityException
import java.security.KeyFactory
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.PrivateKey
import java.security.ProviderException
import java.security.PublicKey
import java.security.Signature
import java.security.SignatureException
import java.security.spec.ECGenParameterSpec
import java.util.Base64
import org.json.JSONObject

@InvokeArg
class KeyArgs {
  var key: String = ""
}

@InvokeArg
class SaveArgs {
  var key: String = ""
  var text: String = ""
}

@InvokeArg
class MsgArgs {
  var msg: String = ""
}

@InvokeArg
class VerifyArgs {
  var msg: String = ""
  var sig: String = ""
}

private class VaultError(message: String) : Exception(message)

private const val PROVIDER = "AndroidKeyStore"
private const val ALIAS = "vault"
private const val ALGORITHM = "SHA256withECDSA"
private const val PREFS = "dev.hoangmirs.vault"
private const val MADE_IN_STRONGBOX = "strongbox"

// A value is at most 64 KiB of text, and JSON escaping can grow it several
// times. A bigger file was not written by this plugin.
private const val FILE_MAX = 1 shl 20

private val KEY_NAME = Regex("[A-Za-z0-9._-]{1,64}")

/**
 * A value is one file in the app's private storage, a key a file. A device key
 * in the Android Keystore signs the values: a StrongBox key when the phone has
 * one, else a key in the trusted environment, else a software key.
 */
@TauriPlugin
class VaultPlugin(private val activity: Activity) : Plugin(activity) {
  private val directory = File(activity.filesDir, "vault")

  // AtomicFile does not guard two writers of one file against each other.
  private val fileLock = Any()

  // Two calls at once would each find no key, and the second would replace the
  // first one's key, because generating under an existing alias replaces it.
  private val keyLock = Any()

  private fun answer(invoke: Invoke, name: String, body: () -> JSObject) {
    val result = try {
      body()
    } catch (e: Exception) {
      invoke.reject("$name failed: ${e.message ?: e.javaClass.simpleName}")
      return
    }
    invoke.resolve(result)
  }

  private fun fileFor(key: String): File {
    if (!KEY_NAME.matches(key)) throw VaultError("a key is letters, digits, '.', '-' or '_'")
    return File(directory, "$key.json")
  }

  private fun decode(text: String): ByteArray? =
    try {
      Base64.getDecoder().decode(text)
    } catch (_: IllegalArgumentException) {
      null
    }

  private fun levelAnswer(level: String?): JSObject =
    JSObject().put("level", level ?: JSONObject.NULL)

  private fun keyStore(): KeyStore = KeyStore.getInstance(PROVIDER).apply { load(null) }

  private fun privateKey(store: KeyStore): PrivateKey? {
    if (!store.containsAlias(ALIAS)) return null
    return store.getKey(ALIAS, null) as? PrivateKey
      ?: throw VaultError("The Keystore holds an entry named $ALIAS that is not a private key")
  }

  @Suppress("DEPRECATION")
  private fun levelOf(key: PrivateKey): String {
    val info = try {
      KeyFactory.getInstance(key.algorithm, PROVIDER).getKeySpec(key, KeyInfo::class.java)
    } catch (_: GeneralSecurityException) {
      // Some phones cannot answer KeyInfo. Rejecting would leave this phone in
      // an error on every read, so claim the lowest level.
      return "software"
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
      return when (info.securityLevel) {
        KeyProperties.SECURITY_LEVEL_STRONGBOX -> "strongbox"
        KeyProperties.SECURITY_LEVEL_TRUSTED_ENVIRONMENT -> "hardware"
        else -> "software"
      }
    }
    if (!info.isInsideSecureHardware) return "software"
    // Before Android 12 KeyInfo cannot tell StrongBox from the trusted
    // environment, so the level makeKey recorded decides.
    val strongBox = Build.VERSION.SDK_INT >= Build.VERSION_CODES.P &&
      activity.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(MADE_IN_STRONGBOX, false)
    return if (strongBox) "strongbox" else "hardware"
  }

  private fun generate(strongBox: Boolean): PrivateKey {
    val spec = KeyGenParameterSpec.Builder(
      ALIAS,
      KeyProperties.PURPOSE_SIGN or KeyProperties.PURPOSE_VERIFY,
    )
      .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
      .setDigests(KeyProperties.DIGEST_SHA256)
      .setUserAuthenticationRequired(false)
    if (strongBox && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
      spec.setIsStrongBoxBacked(true)
    }
    val generator = KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, PROVIDER)
    generator.initialize(spec.build())
    return generator.generateKeyPair().private
  }

  private fun rememberStrongBox(made: Boolean) {
    activity.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
      .edit().putBoolean(MADE_IN_STRONGBOX, made).commit()
  }

  /** Null when this phone cannot make a key at all. */
  private fun makeNewKey(): PrivateKey? {
    try {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
        try {
          val key = generate(strongBox = true)
          rememberStrongBox(true)
          return key
        } catch (_: StrongBoxUnavailableException) {
        }
      }
      val key = generate(strongBox = false)
      rememberStrongBox(false)
      return key
    } catch (_: GeneralSecurityException) {
      return null
    } catch (_: ProviderException) {
      return null
    }
  }

  private fun readValue(key: String): JSObject {
    synchronized(fileLock) {
      val file = fileFor(key)
      val unreadable = JSObject().put("unreadable", true)
      if (file.isDirectory || file.length() > FILE_MAX) return unreadable
      val bytes = try {
        AtomicFile(file).readFully()
      } catch (_: FileNotFoundException) {
        return if (file.exists()) unreadable else JSObject()
      } catch (_: IOException) {
        return unreadable
      }
      val text = try {
        Charsets.UTF_8.newDecoder()
          .onMalformedInput(CodingErrorAction.REPORT)
          .onUnmappableCharacter(CodingErrorAction.REPORT)
          .decode(ByteBuffer.wrap(bytes))
          .toString()
      } catch (_: CharacterCodingException) {
        return unreadable
      }
      return JSObject().put("text", text)
    }
  }

  private fun writeValue(key: String, text: String) {
    synchronized(fileLock) {
      val file = fileFor(key)
      if (!directory.isDirectory && !directory.mkdirs()) {
        throw VaultError("The vault folder cannot be made")
      }
      // AtomicFile would move a directory aside as its backup.
      if (file.isDirectory) throw VaultError("The path of the value is a folder")
      val atomic = AtomicFile(file)
      val out = atomic.startWrite()
      try {
        out.write(text.toByteArray(Charsets.UTF_8))
        atomic.finishWrite(out)
      } catch (e: Exception) {
        atomic.failWrite(out)
        throw e
      }
    }
  }

  @Command
  fun load(invoke: Invoke) {
    answer(invoke, "load") { readValue(invoke.parseArgs(KeyArgs::class.java).key) }
  }

  @Command
  fun save(invoke: Invoke) {
    answer(invoke, "save") {
      val args = invoke.parseArgs(SaveArgs::class.java)
      writeValue(args.key, args.text)
      JSObject()
    }
  }

  @Command
  fun keyLevel(invoke: Invoke) {
    answer(invoke, "keyLevel") {
      levelAnswer(privateKey(keyStore())?.let { levelOf(it) })
    }
  }

  @Command
  fun makeKey(invoke: Invoke) {
    answer(invoke, "makeKey") {
      synchronized(keyLock) {
        val store = keyStore()
        val key = privateKey(store) ?: makeNewKey()
        levelAnswer(key?.let { levelOf(it) })
      }
    }
  }

  @Command
  fun sign(invoke: Invoke) {
    answer(invoke, "sign") {
      val args = invoke.parseArgs(MsgArgs::class.java)
      val msg = decode(args.msg) ?: throw VaultError("sign needs a base64 msg")
      val key = privateKey(keyStore()) ?: throw VaultError("sign found no key")
      val signature = Signature.getInstance(ALGORITHM)
      signature.initSign(key)
      signature.update(msg)
      JSObject().put("sig", Base64.getEncoder().encodeToString(signature.sign()))
    }
  }

  @Command
  fun verify(invoke: Invoke) {
    answer(invoke, "verify") {
      val args = invoke.parseArgs(VerifyArgs::class.java)
      val msg = decode(args.msg) ?: throw VaultError("verify needs a base64 msg")
      val publicKey = publicKey(keyStore()) ?: throw VaultError("verify found no key")
      val sig = decode(args.sig) ?: return@answer JSObject().put("ok", false)
      val signature = Signature.getInstance(ALGORITHM)
      signature.initVerify(publicKey)
      signature.update(msg)
      val ok = try {
        signature.verify(sig)
      } catch (_: SignatureException) {
        false
      }
      JSObject().put("ok", ok)
    }
  }

  private fun publicKey(store: KeyStore): PublicKey? {
    if (!store.containsAlias(ALIAS)) return null
    return store.getCertificate(ALIAS)?.publicKey
      ?: throw VaultError("The Keystore holds no certificate named $ALIAS")
  }
}
