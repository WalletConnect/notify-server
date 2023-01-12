import axios from 'axios'

declare let process: {
  env: {
    JEST_ENV: string,
    TEST_TENANT_ID_APNS: string,
  }
}

const BASE_URLS = new Map<string, string>([
  ['prod', 'https://cast.walletconnect.com'],
  ['staging', 'https://staging.cast.walletconnect.com'],
  ['dev', 'https://dev.cast.walletconnect.com'],
  ['local', 'http://localhost:3000'],
])

const TEST_TENANT = process.env.TEST_TENANT_ID_APNS

const BASE_URL = BASE_URLS.get(process.env.JEST_ENV)

describe('cast', () => {
  describe('Health', () => {
    const url = `${BASE_URL}/health`

    it('is healthy', async () => {
      const { status } = await axios.get(`${url}`)

      expect(status).toBe(200)
    })
  })
})
