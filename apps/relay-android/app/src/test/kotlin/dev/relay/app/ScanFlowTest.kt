package dev.relay.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ScanFlowTest {
	private val done = mutableListOf<Pair<ULong, ScanResult>>()
	private val camera = PermissionGate()
	private val flow = ScanFlow(camera) { id, result -> done += id to result }

	private fun finished(id: ULong = 1uL) = done.single { it.first == id }.second

	@Test
	fun grantedCameraOpensTheScannerAndPassesTheTextOn() {
		assertEquals(ScanStep.Launch, flow.begin(1uL, hasCamera = true, granted = true))
		assertTrue(flow.active)
		flow.scanned("relay://10.0.0.9:7001#ab")
		assertEquals(ScanResult.Text("relay://10.0.0.9:7001#ab"), finished())
		assertFalse(flow.active)
	}

	@Test
	fun theTextIsHandedOverUntouchedForRustToCheck() {
		flow.begin(1uL, true, true)
		flow.scanned("https://example.com")
		assertEquals(ScanResult.Text("https://example.com"), finished())
	}

	@Test
	fun theFirstTapAsksAndAGrantContinuesToTheScanner() {
		assertEquals(ScanStep.Ask, flow.begin(1uL, true, granted = false))
		assertEquals(Gate.REQUESTING, camera.state)
		assertTrue(done.isEmpty())
		assertEquals(ScanStep.Launch, flow.permissionAnswered(granted = true, canAskAgain = true))
		flow.scanned("x")
		assertEquals(ScanResult.Text("x"), finished())
	}

	@Test
	fun aRefusalIsDeniedThenBlockedAndTheNextTapAsksAgainOnlyWhileTheSystemWill() {
		flow.begin(1uL, true, false)
		assertEquals(ScanStep.Finish(ScanResult.CameraDenied), flow.permissionAnswered(false, canAskAgain = true))
		assertEquals(ScanResult.CameraDenied, finished(1uL))
		assertEquals(ScanStep.Ask, flow.begin(2uL, true, false))
		assertEquals(ScanStep.Finish(ScanResult.CameraBlocked), flow.permissionAnswered(false, canAskAgain = false))
		assertEquals(Gate.BLOCKED, camera.state)
		// Blocked: no dialog, straight to the answer that offers the settings.
		assertEquals(ScanStep.Finish(ScanResult.CameraBlocked), flow.begin(3uL, true, false))
		assertEquals(ScanResult.CameraBlocked, finished(3uL))
	}

	@Test
	fun grantingInTheSettingsUnblocksTheNextTap() {
		flow.begin(1uL, true, false)
		flow.permissionAnswered(false, false)
		assertEquals(ScanStep.Launch, flow.begin(2uL, true, granted = true))
	}

	@Test
	fun theCameraGateIsNotTheMicrophonesAndIsUntouchedUntilTheFirstTap() {
		assertEquals(Gate.UNKNOWN, camera.state)
		assertTrue(done.isEmpty())
	}

	@Test
	fun aCancelledScanIsNotAnError() {
		flow.begin(1uL, true, true)
		flow.scanned(null)
		assertEquals(ScanResult.Cancelled, finished())
		flow.begin(2uL, true, true)
		flow.scanned("  ")
		assertEquals(ScanResult.Cancelled, finished(2uL))
	}

	@Test
	fun aScannerFailureCarriesItsReason() {
		flow.begin(1uL, true, true)
		flow.scanned(null, error = "CameraUnavailableException")
		assertEquals(ScanResult.Failed("CameraUnavailableException"), finished())
	}

	@Test
	fun noCameraMeansUnavailable() {
		assertEquals(ScanStep.Finish(ScanResult.Unavailable), flow.begin(1uL, hasCamera = false, granted = true))
		assertEquals(ScanResult.Unavailable, finished())
		assertFalse(flow.active)
	}

	@Test
	fun aSecondRequestWhileOneIsOpenIsRefusedAndTheFirstSurvives() {
		flow.begin(1uL, true, true)
		assertTrue(flow.begin(2uL, true, true) is ScanStep.Finish)
		assertTrue(finished(2uL) is ScanResult.Failed)
		assertTrue(flow.active)
		// The activity was recreated meanwhile: the answer still reaches request 1.
		flow.scanned("kept")
		assertEquals(ScanResult.Text("kept"), finished(1uL))
	}
}
